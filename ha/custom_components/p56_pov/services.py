"""Actions: p56_pov.show_text, show_image, show_ai, play_file, delete_file."""

from __future__ import annotations

from typing import Any

import voluptuous as vol

from homeassistant.config_entries import ConfigEntryState
from homeassistant.const import ATTR_DEVICE_ID
from homeassistant.core import HomeAssistant, ServiceCall, ServiceResponse, SupportsResponse
from homeassistant.exceptions import ServiceValidationError
from homeassistant.helpers import config_validation as cv, device_registry as dr
from homeassistant.util import dt as dt_util

from .const import (
    AI_KINDS,
    CONF_BACKGROUND,
    CONF_COLOR,
    CONF_MODE,
    DEFAULT_BACKGROUND,
    DEFAULT_COLOR,
    DEFAULT_MODE,
    DOMAIN,
    MODES,
    STYLES,
)
from .coordinator import P56ConfigEntry, P56Coordinator
from .entity import run_command
from .show import b64, color_value, image_bytes, is_video, video_frames

SHOW_COMMON = {
    vol.Optional(ATTR_DEVICE_ID): vol.All(cv.ensure_list, [cv.string]),
    vol.Optional("show_seconds"): vol.All(vol.Coerce(float), vol.Range(min=0, max=86400)),
    vol.Optional("mode"): vol.In(MODES),
    vol.Optional("name"): vol.All(cv.string, vol.Length(min=1, max=40)),
    vol.Optional("priority"): vol.All(vol.Coerce(int), vol.Range(min=-100, max=100)),
}

COLOR = vol.Any(cv.string, vol.All(vol.ExactSequence([cv.byte, cv.byte, cv.byte])))

SHOW_TEXT_SCHEMA = vol.All(
    vol.Schema(
        {
            **SHOW_COMMON,
            vol.Optional("text"): vol.All(cv.string, vol.Length(max=300)),
            vol.Optional("color"): COLOR,
            vol.Optional("background"): COLOR,
            vol.Optional("font_size"): vol.All(vol.Coerce(float), vol.Range(min=0, max=400)),
            vol.Optional("icon"): cv.string,
            vol.Optional("icon_color"): COLOR,
            vol.Optional("style"): vol.In(STYLES),
            vol.Optional("ring"): cv.boolean,
            vol.Optional("value"): vol.Coerce(float),
            vol.Optional("value_min"): vol.Coerce(float),
            vol.Optional("value_max"): vol.Coerce(float),
            vol.Optional("invert_colors"): cv.boolean,
            vol.Optional("gauge"): cv.boolean,
        }
    ),
    cv.has_at_least_one_key("text", "icon", "style"),
)

SHOW_IMAGE_SCHEMA = vol.Schema(
    {
        **SHOW_COMMON,
        vol.Optional("path"): cv.string,
        vol.Optional("url"): cv.url,
        vol.Optional("camera"): cv.entity_domain("camera"),
        vol.Optional("media"): vol.Any(cv.string, dict),
        vol.Optional("scale"): vol.All(vol.Coerce(float), vol.Range(min=0.1, max=5)),
        vol.Optional("x_offset"): vol.All(vol.Coerce(float), vol.Range(min=-1, max=1)),
        vol.Optional("y_offset"): vol.All(vol.Coerce(float), vol.Range(min=-1, max=1)),
        vol.Optional("mirror"): cv.boolean,
        vol.Optional("max_frames"): vol.All(vol.Coerce(int), vol.Range(min=1, max=600)),
        vol.Optional("fps"): vol.All(vol.Coerce(float), vol.Range(min=1, max=30)),
        # text over the picture (every frame of an animation), e.g. over a rain radar
        vol.Optional("text"): vol.All(cv.string, vol.Length(max=300)),
        vol.Optional("color"): COLOR,
        vol.Optional("font_size"): vol.All(vol.Coerce(float), vol.Range(min=0, max=400)),
    }
)

SHOW_AI_SCHEMA = vol.Schema(
    {
        **SHOW_COMMON,
        vol.Required("prompt"): vol.All(cv.string, vol.Length(min=1, max=1000)),
        vol.Optional("kind"): vol.In(AI_KINDS),
        vol.Optional("art_style"): vol.All(cv.string, vol.Length(max=80)),
        # "comfyui": ComfyUI on a computer in the house (add-on option comfyui_url)
        vol.Optional("entity_id"): vol.Any("comfyui", cv.entity_domain("ai_task")),
        vol.Optional("fresh"): cv.boolean,
        # text over the AI's background (then the AI makes a background, not a subject)
        vol.Optional("text"): vol.All(cv.string, vol.Length(max=300)),
        vol.Optional("color"): COLOR,
        vol.Optional("font_size"): vol.All(vol.Coerce(float), vol.Range(min=0, max=400)),
        vol.Optional("icon"): cv.string,
        vol.Optional("value"): vol.Coerce(float),
        vol.Optional("value_min"): vol.Coerce(float),
        vol.Optional("value_max"): vol.Coerce(float),
        vol.Optional("invert_colors"): cv.boolean,
        vol.Optional("gauge"): cv.boolean,
    }
)

FILE_SCHEMA = vol.All(
    vol.Schema(
        {
            vol.Required(ATTR_DEVICE_ID): vol.All(cv.ensure_list, [cv.string]),
            vol.Optional("slot"): vol.All(vol.Coerce(int), vol.Range(min=1, max=255)),
            vol.Optional("name"): cv.string,
        }
    ),
    cv.has_at_least_one_key("slot", "name"),
)


def _loaded(hass: HomeAssistant) -> list[P56ConfigEntry]:
    return [e for e in hass.config_entries.async_entries(DOMAIN) if e.state is ConfigEntryState.LOADED]


def _targets(hass: HomeAssistant, call: ServiceCall) -> list[tuple[P56Coordinator, str | None]]:
    """(coordinator, fan MAC) per chosen device; no devices = every fan of every server."""
    entries = _loaded(hass)
    if not entries:
        raise ServiceValidationError(translation_domain=DOMAIN, translation_key="not_loaded")
    device_ids = call.data.get(ATTR_DEVICE_ID) or []
    if not device_ids:
        return [(e.runtime_data, None) for e in entries]
    registry = dr.async_get(hass)
    out: list[tuple[P56Coordinator, str | None]] = []
    for device_id in device_ids:
        device = registry.async_get(device_id)
        macs = [i[1] for i in device.identifiers if i[0] == DOMAIN] if device else []
        entry = next((e for e in entries if device and e.entry_id == device.config_entry_id), None)
        if not macs or entry is None:
            raise ServiceValidationError(
                translation_domain=DOMAIN, translation_key="unknown_device", translation_placeholders={"device": device_id}
            )
        mac = macs[0]
        out.append((entry.runtime_data, None if mac == entry.entry_id else mac))
    return out


async def _show(call: ServiceCall, payload: dict[str, Any], default_colors: bool = False, ai: bool = False) -> ServiceResponse:
    """Send the payload to every target; options defaults come from each target's own entry.

    default_colors: fill a missing color/background from the target entry's options.
    ai: the payload is a prompt; the server has the picture made by the AI Task first.
    """
    results: list[dict[str, Any]] = []
    for coordinator, mac in _targets(call.hass, call):
        body = dict(payload)
        opts = coordinator.config_entry.options
        body.setdefault("mode", opts.get(CONF_MODE, DEFAULT_MODE))
        if default_colors:
            body.setdefault("color", color_value(opts.get(CONF_COLOR, DEFAULT_COLOR)))
            body.setdefault("background", color_value(opts.get(CONF_BACKGROUND, DEFAULT_BACKGROUND)))
        if mac:
            body["mac"] = mac
        send = coordinator.api.show_ai if ai else coordinator.api.show
        answer = await run_command(coordinator, send(body))
        results.append(answer)
    return {"results": results}


async def _show_text(call: ServiceCall) -> ServiceResponse:
    data = call.data
    style = data.get("style")
    text = data.get("text", "")
    if style == "clock" and not text.strip():
        text = dt_util.now().strftime("%H:%M")
    payload: dict[str, Any] = {"text": text}
    # Explicit colours always win. Missing ones are filled per target from that target's
    # own entry options (in _show), except with a style, which brings its own colours.
    for key in ("color", "background"):
        if key in data:
            payload[key] = color_value(data[key])
    if "value" in data:
        payload["color_scale"] = _color_scale(data)
    if "icon_color" in data:
        payload["icon_color"] = color_value(data["icon_color"])
    for key in ("icon", "style", "ring", "gauge", "font_size", "show_seconds", "mode", "name", "priority"):
        if key in data:
            payload[key] = data[key]
    return await _show(call, payload, default_colors=not style)


def _color_scale(data: dict[str, Any]) -> dict[str, Any]:
    """Text colour from `value` between `value_min` (green) and `value_max` (red)."""
    if "value_min" not in data or "value_max" not in data:
        raise ServiceValidationError(translation_domain=DOMAIN, translation_key="value_needs_range")
    return {
        "value": data["value"],
        "min": data["value_min"],
        "max": data["value_max"],
        "invert": data.get("invert_colors", False),
    }


async def _show_image(call: ServiceCall) -> ServiceResponse:
    data = await image_bytes(call.hass, call.data)
    payload: dict[str, Any] = {}
    if is_video(data):
        fps = call.data.get("fps", 10.0)
        frames = await video_frames(call.hass, data, fps, call.data.get("max_frames", 100))
        payload["frames"] = [b64(f) for f in frames]
        payload["frame_ms"] = round(1000 / fps)
    else:
        payload["data"] = b64(data)
    for key in ("show_seconds", "mode", "name", "max_frames", "priority", "text", "font_size"):
        if key in call.data:
            payload[key] = call.data[key]
    if "color" in call.data:
        payload["color"] = color_value(call.data["color"])
    if any(k in call.data for k in ("scale", "x_offset", "y_offset", "mirror")):
        payload["placement"] = {
            "scale": call.data.get("scale", 1.0),
            "x": call.data.get("x_offset", 0.0),
            "y": call.data.get("y_offset", 0.0),
            "mirror": call.data.get("mirror", False),
        }
    return await _show(call, payload)


async def _show_ai(call: ServiceCall) -> ServiceResponse:
    keys = ("prompt", "kind", "art_style", "entity_id", "fresh", "text", "icon", "font_size", "gauge", "show_seconds", "mode", "name", "priority")
    payload = {k: call.data[k] for k in keys if k in call.data}
    if "color" in call.data:
        payload["color"] = color_value(call.data["color"])
    if "value" in call.data:
        payload["color_scale"] = _color_scale(call.data)
    return await _show(call, payload, ai=True)


async def _file_action(call: ServiceCall, what: str) -> None:
    for coordinator, mac in _targets(call.hass, call):
        fan = coordinator.fan(mac) if mac else None
        if fan is None:
            raise ServiceValidationError(translation_domain=DOMAIN, translation_key="fan_offline")
        slot = call.data.get("slot")
        if (name := call.data.get("name")) is not None:
            wanted = name if name.lower().endswith(".mp4") else f"{name}.mp4"
            slot = next((s for s, n in fan.files if n.lower() == wanted.lower()), None)
            if slot is None:
                raise ServiceValidationError(
                    translation_domain=DOMAIN, translation_key="file_not_on_fan", translation_placeholders={"name": name}
                )
        await run_command(coordinator, coordinator.api.set_value(fan.id, what, int(slot)))


async def _play_file(call: ServiceCall) -> None:
    await _file_action(call, "select")


async def _delete_file(call: ServiceCall) -> None:
    await _file_action(call, "delete")


def async_setup_services(hass: HomeAssistant) -> None:
    hass.services.async_register(
        DOMAIN, "show_text", _show_text, schema=SHOW_TEXT_SCHEMA, supports_response=SupportsResponse.OPTIONAL
    )
    hass.services.async_register(
        DOMAIN, "show_image", _show_image, schema=SHOW_IMAGE_SCHEMA, supports_response=SupportsResponse.OPTIONAL
    )
    hass.services.async_register(
        DOMAIN, "show_ai", _show_ai, schema=SHOW_AI_SCHEMA, supports_response=SupportsResponse.OPTIONAL
    )
    hass.services.async_register(DOMAIN, "play_file", _play_file, schema=FILE_SCHEMA)
    hass.services.async_register(DOMAIN, "delete_file", _delete_file, schema=FILE_SCHEMA)
