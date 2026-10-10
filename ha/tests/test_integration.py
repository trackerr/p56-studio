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
    await wait_for(hass, "fan entities", lambda: hass.states.get("light.virtuele_p56") is not None)
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
    light = "light.virtuele_p56"
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
        DOMAIN, "show_text", {"text": "0,23 €/kWh", "color": [255, 200, 0], "name": "prijs"},
        blocking=True, return_response=True,
    )
    assert resp["results"][0]["success"] is True
    files = entity_id(hass, "sensor", "file_count")
    select = entity_id(hass, "select", "file")
    await wait_for(hass, "text file", lambda: "prijs.mp4" in hass.states.get(select).attributes.get("options", []))
    await wait_for(hass, "plays text", lambda: hass.states.get(select).state == "prijs.mp4")

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
        DOMAIN, "show_image", {"device_id": device.id, "path": str(pic), "name": "kort", "show_seconds": 1}, blocking=True
    )
    await wait_for(hass, "picture shown", lambda: "kort.mp4" in hass.states.get(select).attributes.get("options", []))
    await wait_for(hass, "picture gone again", lambda: "kort.mp4" not in hass.states.get(select).attributes.get("options", []), 20)

    # play / delete by name
    await hass.services.async_call(DOMAIN, "delete_file", {"device_id": device.id, "name": "prijs"}, blocking=True)
    await wait_for(hass, "deleted", lambda: "prijs.mp4" not in hass.states.get(select).attributes.get("options", []))
    count_before = int(hass.states.get(files).state)
    before_opts = hass.states.get(select).attributes.get("options")

    # text entity replaces its own file
    text = entity_id(hass, "text", "text")
    await hass.services.async_call("text", "set_value", {"entity_id": text, "value": "hallo"}, blocking=True)
    await hass.services.async_call("text", "set_value", {"entity_id": text, "value": "wereld"}, blocking=True)
    await wait_for(hass, "one text file", lambda: int(hass.states.get(files).state) == count_before + 1, msg=lambda: (count_before, before_opts, hass.states.get(files).state, hass.states.get(select).attributes.get("options")))

    # notify entity
    notify = entity_id(hass, "notify", "message")
    await hass.services.async_call("notify", "send_message", {"entity_id": notify, "message": "Wasmachine klaar"}, blocking=True)
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
    await wait_for(hass, "reloaded", lambda: entry.state is ConfigEntryState.LOADED and hass.states.get("light.virtuele_p56").state != "unavailable")
    diag = await async_get_config_entry_diagnostics(hass, entry)
    assert diag["server_state"]["devices"][0]["mac"] == "**REDACTED**"
    assert await hass.config_entries.async_unload(entry.entry_id)