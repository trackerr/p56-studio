"""The integration against a real p56-server with a virtual fan."""

from __future__ import annotations

import asyncio
import io
from collections.abc import Callable
from pathlib import Path

import pytest

from homeassistant import config_entries
from homeassistant.config_entries import ConfigEntryState
from homeassistant.const import CONF_HOST, CONF_PORT
from homeassistant.core import HomeAssistant
from homeassistant.data_entry_flow import FlowResultType
from homeassistant.exceptions import HomeAssistantError, ServiceValidationError
from homeassistant.helpers import device_registry as dr, entity_registry as er
from homeassistant.helpers.service_info.hassio import HassioServiceInfo

from pytest_homeassistant_custom_component.common import MockConfigEntry

from custom_components.p56_pov.const import DOMAIN

def png() -> bytes:
    from PIL import Image  # noqa: PLC0415

    buf = io.BytesIO()
    Image.new("RGB", (64, 64), (0, 120, 255)).save(buf, "PNG")
    return buf.getvalue()


async def wait_for(hass: HomeAssistant, what: str, check: Callable[[], bool], timeout: float = 15, msg: Callable[[], object] | None = None) -> None:
    loop = asyncio.get_running_loop()
    end = loop.time() + timeout
    while not check():
        if loop.time() > end:
            pytest.fail(f"timed out: {what} {msg() if msg else str()}")
        await asyncio.sleep(0.1)
        await hass.async_block_till_done()


def entity_id(hass: HomeAssistant, platform: str, key: str) -> str:
    reg = er.async_get(hass)
    found = [e.entity_id for e in reg.entities.values() if e.platform == DOMAIN and e.domain == platform and e.unique_id.endswith(f"_{key}")]
    assert found, f"no {platform} entity {key}"
    return found[0]


async def setup(hass: HomeAssistant, server: tuple[str, int]) -> MockConfigEntry:
    host, port = server
    entry = MockConfigEntry(domain=DOMAIN, version=2, data={CONF_HOST: host, CONF_PORT: port}, unique_id=f"{host}:{port}")
    entry.add_to_hass(hass)
    assert await hass.config_entries.async_setup(entry.entry_id)
    await hass.async_block_till_done()
    await wait_for(hass, "fan entities", lambda: hass.states.get("light.virtual_p56") is not None)
    return entry


async def test_user_flow(hass: HomeAssistant, p56_server) -> None:
    host, port = p56_server
    result = await hass.config_entries.flow.async_init(DOMAIN, context={"source": config_entries.SOURCE_USER})
    assert result["type"] is FlowResultType.FORM
    bad = await hass.config_entries.flow.async_configure(result["flow_id"], {CONF_HOST: "127.0.0.1", CONF_PORT: 1})
    assert bad["errors"] == {"base": "cannot_connect"}
    ok = await hass.config_entries.flow.async_configure(result["flow_id"], {CONF_HOST: host, CONF_PORT: port})
    assert ok["type"] is FlowResultType.CREATE_ENTRY
    assert ok["data"] == {CONF_HOST: host, CONF_PORT: port}
    await hass.async_block_till_done()
    entry = hass.config_entries.async_entries(DOMAIN)[0]
    assert entry.state is ConfigEntryState.LOADED
    assert await hass.config_entries.async_unload(entry.entry_id)


async def test_hassio_discovery(hass: HomeAssistant, p56_server) -> None:
    host, port = p56_server
    info = HassioServiceInfo(
        config={"host": host, "port": port}, name="P56 POV Display", slug="abc123_p56_pov_display", uuid="1234"
    )
    result = await hass.config_entries.flow.async_init(DOMAIN, context={"source": config_entries.SOURCE_HASSIO}, data=info)
    assert result["type"] is FlowResultType.FORM
    assert result["step_id"] == "hassio_confirm"
    done = await hass.config_entries.flow.async_configure(result["flow_id"], {})
    assert done["type"] is FlowResultType.CREATE_ENTRY
    assert done["data"]["addon_slug"] == "abc123_p56_pov_display"
    await hass.async_block_till_done()
    # announced again: no second entry
    again = await hass.config_entries.flow.async_init(DOMAIN, context={"source": config_entries.SOURCE_HASSIO}, data=info)
    assert again["type"] is FlowResultType.ABORT
    entry = hass.config_entries.async_entries(DOMAIN)[0]
    assert await hass.config_entries.async_unload(entry.entry_id)


async def test_entities_and_actions(hass: HomeAssistant, p56_server, tmp_path: Path) -> None:
    entry = await setup(hass, p56_server)
    light = "light.virtual_p56"
    assert hass.states.get(light).state == "on"

    # power and brightness
    await hass.services.async_call("light", "turn_on", {"entity_id": light, "brightness": 128}, blocking=True)
    await wait_for(hass, "brightness 8", lambda: hass.states.get(light).attributes.get("brightness") == 128)
    await hass.services.async_call("light", "turn_off", {"entity_id": light}, blocking=True)
    await wait_for(hass, "off", lambda: hass.states.get(light).state == "off")
    await hass.services.async_call("light", "turn_on", {"entity_id": light}, blocking=True)
    await wait_for(hass, "on", lambda: hass.states.get(light).state == "on")

    # angle
    angle = entity_id(hass, "number", "angle")
    await hass.services.async_call("number", "set_value", {"entity_id": angle, "value": -30}, blocking=True)
    await wait_for(hass, "angle", lambda: hass.states.get(angle).state == "-30")

    # text action with response, stored under its own name
    resp = await hass.services.async_call(
        DOMAIN, "show_text", {"text": "0,23 €/kWh", "color": [255, 200, 0], "name": "price"},
        blocking=True, return_response=True,
    )
    assert resp["results"][0]["success"] is True
    files = entity_id(hass, "sensor", "file_count")
    select = entity_id(hass, "select", "file")
    await wait_for(hass, "text file", lambda: "price.mp4" in hass.states.get(select).attributes.get("options", []))
    await wait_for(hass, "plays text", lambda: hass.states.get(select).state == "price.mp4")

    # image entity shows what was sent
    image = entity_id(hass, "image", "shown")
    await wait_for(hass, "image updated", lambda: hass.states.get(image).state not in ("unknown", "unavailable"))
    shown = hass.data["image"].get_entity(image)
    assert (await shown.async_image())[:8] == b"\x89PNG\r\n\x1a\n"

    # picture from a file, on one device, removed again after 1 s
    pic = tmp_path / "pic.png"
    pic.write_bytes(png())
    hass.config.allowlist_external_dirs = {str(tmp_path)}
    devices = dr.async_entries_for_config_entry(dr.async_get(hass), entry.entry_id)
    device = next(d for d in devices if d.via_device_id)
    assert any(d.id == device.via_device_id for d in devices)
    await hass.services.async_call(
        DOMAIN, "show_image", {"device_id": device.id, "path": str(pic), "name": "short", "show_seconds": 1}, blocking=True
    )
    await wait_for(hass, "picture shown", lambda: "short.mp4" in hass.states.get(select).attributes.get("options", []))
    await wait_for(hass, "picture gone again", lambda: "short.mp4" not in hass.states.get(select).attributes.get("options", []), 20)

    # play / delete by name
    await hass.services.async_call(DOMAIN, "delete_file", {"device_id": device.id, "name": "price"}, blocking=True)
    await wait_for(hass, "deleted", lambda: "price.mp4" not in hass.states.get(select).attributes.get("options", []))
    count_before = int(hass.states.get(files).state)
    before_opts = hass.states.get(select).attributes.get("options")

    # text entity replaces its own file
    text = entity_id(hass, "text", "text")
    await hass.services.async_call("text", "set_value", {"entity_id": text, "value": "hello"}, blocking=True)
    await hass.services.async_call("text", "set_value", {"entity_id": text, "value": "world"}, blocking=True)
    await wait_for(hass, "one text file", lambda: int(hass.states.get(files).state) == count_before + 1, msg=lambda: (count_before, before_opts, hass.states.get(files).state, hass.states.get(select).attributes.get("options")))

    # notify entity
    notify = entity_id(hass, "notify", "message")
    await hass.services.async_call("notify", "send_message", {"entity_id": notify, "message": "Washing machine done"}, blocking=True)
    await wait_for(hass, "notification shown", lambda: "ha_melding.mp4" in hass.states.get(select).attributes.get("options", []))

    # switches
    playing = entity_id(hass, "switch", "playing")
    await hass.services.async_call("switch", "turn_off", {"entity_id": playing}, blocking=True)
    await wait_for(hass, "paused", lambda: hass.states.get(playing).state == "off")

    # a connected fan and the server device cannot be removed; a gone fan can
    from custom_components.p56_pov import async_remove_config_entry_device  # noqa: PLC0415

    server = next(d for d in devices if not d.via_device_id)
    assert not await async_remove_config_entry_device(hass, entry, device)
    assert not await async_remove_config_entry_device(hass, entry, server)
    gone = dr.async_get(hass).async_get_or_create(config_entry_id=entry.entry_id, identifiers={(DOMAIN, "aa:bb:cc:dd:ee:ff")})
    assert await async_remove_config_entry_device(hass, entry, gone)

    # server sensor + live events
    conn = entity_id(hass, "sensor", "fans_connected")
    assert hass.states.get(conn).state == "1"
    await wait_for(hass, "live events", lambda: entry.runtime_data.live)

    assert await hass.config_entries.async_unload(entry.entry_id)
    await hass.async_block_till_done()
    assert entry.state is ConfigEntryState.NOT_LOADED


async def test_discovery_adopts_manual_localhost_entry(hass: HomeAssistant, p56_server) -> None:
    _, port = p56_server
    entry = MockConfigEntry(domain=DOMAIN, version=1, data={CONF_HOST: "localhost", CONF_PORT: port, "name": "P56 POV Display"})
    entry.add_to_hass(hass)
    assert await hass.config_entries.async_setup(entry.entry_id)
    await hass.async_block_till_done()
    info = HassioServiceInfo(config={"host": "127.0.0.1", "port": port}, name="P56 POV Display", slug="5e97629b_p56_pov_display", uuid="1")
    result = await hass.config_entries.flow.async_init(DOMAIN, context={"source": config_entries.SOURCE_HASSIO}, data=info)
    assert result["type"] is FlowResultType.ABORT
    await hass.async_block_till_done()
    assert len(hass.config_entries.async_entries(DOMAIN)) == 1
    assert entry.data["addon_slug"] == "5e97629b_p56_pov_display"
    assert entry.version == 2
    assert await hass.config_entries.async_unload(entry.entry_id)


async def test_server_down(hass: HomeAssistant) -> None:
    entry = MockConfigEntry(domain=DOMAIN, version=2, data={CONF_HOST: "127.0.0.1", CONF_PORT: 1})
    entry.add_to_hass(hass)
    await hass.config_entries.async_setup(entry.entry_id)
    await hass.async_block_till_done()
    assert entry.state is ConfigEntryState.SETUP_RETRY


async def test_migrate_v1(hass: HomeAssistant, p56_server) -> None:
    host, port = p56_server
    entry = MockConfigEntry(domain=DOMAIN, version=1, data={CONF_HOST: host, CONF_PORT: port})
    entry.add_to_hass(hass)
    assert await hass.config_entries.async_setup(entry.entry_id)
    await hass.async_block_till_done()
    assert entry.version == 2
    assert entry.unique_id == f"{host}:{port}"
    assert await hass.config_entries.async_unload(entry.entry_id)


async def test_options_and_diagnostics(hass: HomeAssistant, p56_server) -> None:
    from custom_components.p56_pov.diagnostics import async_get_config_entry_diagnostics  # noqa: PLC0415

    entry = await setup(hass, p56_server)
    flow = await hass.config_entries.options.async_init(entry.entry_id)
    assert flow["type"] is FlowResultType.FORM
    done = await hass.config_entries.options.async_configure(
        flow["flow_id"],
        {"mode": "cast", "show_seconds": 5, "notify_seconds": 20, "color": "#00ff00", "background": "black"},
    )
    assert done["type"] is FlowResultType.CREATE_ENTRY
    await hass.async_block_till_done()
    entry = hass.config_entries.async_get_entry(entry.entry_id)
    assert entry.options["mode"] == "cast"
    await wait_for(hass, "reloaded", lambda: entry.state is ConfigEntryState.LOADED and hass.states.get("light.virtual_p56").state != "unavailable")
    diag = await async_get_config_entry_diagnostics(hass, entry)
    assert diag["server_state"]["devices"][0]["mac"] == "**REDACTED**"
    assert await hass.config_entries.async_unload(entry.entry_id)

async def test_follows_virtual_fan_and_addon_restarts(hass: HomeAssistant, tmp_path: Path) -> None:
    """Switching the virtual fan and restarting the add-on show up within seconds."""
    import socket  # noqa: PLC0415

    from .conftest import free_port, start_server  # noqa: PLC0415

    opts = tmp_path / "options.json"
    opts.write_text('{"simulate": true}')
    http, tcp, udp = free_port(), free_port(), free_port(socket.SOCK_DGRAM)
    proc = start_server(http, tcp, udp, opts)
    try:
        loop = asyncio.get_running_loop()
        end = loop.time() + 20
        while True:
            try:
                await asyncio.wait_for(asyncio.open_connection("127.0.0.1", http), 1)
                break
            except OSError:
                assert loop.time() < end, "server did not start"
                await asyncio.sleep(0.2)
        entry = MockConfigEntry(domain=DOMAIN, version=2, data={CONF_HOST: "127.0.0.1", CONF_PORT: http})
        entry.add_to_hass(hass)
        assert await hass.config_entries.async_setup(entry.entry_id)
        light = "light.virtual_p56"
        state = lambda: (hass.states.get(light) or type("S", (), {"state": "missing"})).state  # noqa: E731
        await wait_for(hass, "fan on", lambda: state() == "on", 20)
        await wait_for(hass, "live events", lambda: entry.runtime_data.live)

        opts.write_text('{"simulate": false}')
        await wait_for(hass, "virtual fan off -> unavailable", lambda: state() == "unavailable", 8)
        opts.write_text('{"simulate": true}')
        await wait_for(hass, "virtual fan on -> available", lambda: state() == "on", 10)

        proc.kill()
        proc.wait()
        await wait_for(hass, "add-on stopped -> unavailable", lambda: state() == "unavailable", 5)
        proc = start_server(http, tcp, udp, opts)
        await wait_for(hass, "add-on back -> available", lambda: state() == "on", 20)

        await hass.services.async_call("light", "turn_off", {"entity_id": light}, blocking=True)
        await wait_for(hass, "turn off after restart", lambda: state() == "off", 5)
        await hass.services.async_call("light", "turn_on", {"entity_id": light}, blocking=True)
        await wait_for(hass, "turn on after restart", lambda: state() == "on", 5)
        assert await hass.config_entries.async_unload(entry.entry_id)
    finally:
        proc.kill()
        proc.wait()

def gif_bytes(colors: list[tuple[int, int, int]]) -> bytes:
    from PIL import Image  # noqa: PLC0415

    frames = [Image.new("RGB", (64, 64), c) for c in colors]
    buf = io.BytesIO()
    frames[0].save(buf, "GIF", save_all=True, append_images=frames[1:], duration=80, loop=0)
    return buf.getvalue()


async def test_styles_icons_values_and_animations(hass: HomeAssistant, p56_server, tmp_path: Path) -> None:
    entry = await setup(hass, p56_server)
    # style + icon + colour from a value
    resp = await hass.services.async_call(
        DOMAIN, "show_text",
        {"text": "0,31 €/kWh", "style": "price", "value": 0.31, "value_min": 0.2, "value_max": 0.35, "name": "price"},
        blocking=True, return_response=True,
    )
    assert resp["results"][0]["success"] is True
    # icon alone, and the clock style without text (shows the time)
    for data in ({"icon": "mdi:washing-machine", "name": "icon"}, {"style": "clock", "name": "clock"}):
        resp = await hass.services.async_call(DOMAIN, "show_text", data, blocking=True, return_response=True)
        assert resp["results"][0]["success"] is True, data
    # an animated GIF keeps its frames
    gif = tmp_path / "anim.gif"
    gif.write_bytes(gif_bytes([(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0)]))
    hass.config.allowlist_external_dirs = {str(tmp_path)}
    resp = await hass.services.async_call(
        DOMAIN, "show_image", {"path": str(gif), "name": "anim"}, blocking=True, return_response=True
    )
    assert resp["results"][0]["results"][0]["frames"] == 4
    # a value needs its range
    with pytest.raises(ServiceValidationError):
        await hass.services.async_call(DOMAIN, "show_text", {"text": "x", "value": 3}, blocking=True)
    # an unknown icon is reported by the server
    with pytest.raises(HomeAssistantError):
        await hass.services.async_call(DOMAIN, "show_text", {"text": "x", "icon": "mdi:not-an-icon"}, blocking=True)
    select = entity_id(hass, "select", "file")
    await wait_for(hass, "files", lambda: {"price.mp4", "icon.mp4", "clock.mp4", "anim.mp4"} <= set(hass.states.get(select).attributes.get("options", [])))
    assert await hass.config_entries.async_unload(entry.entry_id)


def test_video_helpers() -> None:
    from custom_components.p56_pov.show import is_video, split_jpegs  # noqa: PLC0415

    assert is_video(b"\x00\x00\x00\x18ftypmp42" + b"\x00" * 8)
    assert is_video(b"\x1a\x45\xdf\xa3" + b"\x00" * 12)
    assert not is_video(b"GIF89a" + b"\x00" * 10)
    stream = b"\xff\xd8aaa\xff\xd9" + b"\xff\xd8bb\xff\xd9" + b"\xff\xd8c"
    assert split_jpegs(stream) == [b"\xff\xd8aaa\xff\xd9", b"\xff\xd8bb\xff\xd9"]

async def test_video_becomes_an_animation(hass: HomeAssistant, p56_server, tmp_path: Path, monkeypatch) -> None:
    """A video goes through ffmpeg (faked here) and reaches the fan as frames."""
    from PIL import Image  # noqa: PLC0415

    from custom_components.p56_pov import show as show_mod  # noqa: PLC0415

    def jpeg(c):
        buf = io.BytesIO()
        Image.new("RGB", (96, 96), c).save(buf, "JPEG")
        return buf.getvalue()

    calls = {}

    class FakeProc:
        async def communicate(self):
            return jpeg((255, 0, 0)) + jpeg((0, 0, 255)) + jpeg((0, 255, 0)), b""

        def kill(self):
            pass

    async def fake_exec(*args, **kwargs):
        calls["args"] = args
        return FakeProc()

    monkeypatch.setattr(show_mod, "_ffmpeg_binary", lambda hass: "ffmpeg")
    monkeypatch.setattr(show_mod.asyncio, "create_subprocess_exec", fake_exec)
    entry = await setup(hass, p56_server)
    video = tmp_path / "clip.mp4"
    video.write_bytes(b"\x00\x00\x00\x18ftypmp42" + b"\x00" * 64)
    hass.config.allowlist_external_dirs = {str(tmp_path)}
    resp = await hass.services.async_call(
        DOMAIN, "show_image", {"path": str(video), "name": "clip", "fps": 5, "max_frames": 3}, blocking=True, return_response=True
    )
    assert resp["results"][0]["results"][0]["frames"] == 3
    assert "fps=5.0" in " ".join(calls["args"]) and "3" in calls["args"]
    assert await hass.config_entries.async_unload(entry.entry_id)