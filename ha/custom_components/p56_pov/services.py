"""Actions: p56_pov.show_text, show_image, play_file, delete_file."""

from __future__ import annotations

from typing import Any

import voluptuous as vol

from homeassistant.config_entries import ConfigEntryState
from homeassistant.const import ATTR_DEVICE_ID
from homeassistant.core import HomeAssistant, ServiceCall, ServiceResponse, SupportsResponse
from homeassistant.exceptions import ServiceValidationError
from homeassistant.helpers import config_validation as cv, device_registry as dr

from .const import CONF_BACKGROUND, CONF_COLOR, CONF_MODE, DEFAULT_BACKGROUND, DEFAULT_COLOR, DEFAULT_MODE, DOMAIN, MODES
from .coordinator import P56ConfigEntry, P56Coordinator
from .entity import run_command
from .show import b64, color_value, image_bytes

SHOW_COMMON = {
    vol.Optional(ATTR_DEVICE_ID): vol.All(cv.ensure_list, [cv.string]),
    vol.Optional("show_seconds"): vol.All(vol.Coerce(float), vol.Range(min=0, max=86400)),
    vol.Optional("mode"): vol.In(MODES),
    vol.Optional("name"): vol.All(cv.string, vol.Length(min=1, max=40)),
}

SHOW_TEXT_SCHEMA = vol.Schema(
    {
        **SHOW_COMMON,
        vol.Required("text"): vol.All(cv.string, vol.Length(min=1, max=300)),
        vol.Optional("color"): vol.Any(cv.string, vol.All(vol.ExactSequence([cv.byte, cv.byte, cv.byte]))),
        vol.Optional("background"): vol.Any(cv.string, vol.All(vol.ExactSequence([cv.byte, cv.byte, cv.byte]))),
        vol.Optional("font_size"): vol.All(vol.Coerce(float), vol.Range(min=0, max=400)),
    }
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


async def _show(call: ServiceCall, payload: dict[str, Any]) -> ServiceResponse:
    results: list[dict[str, Any]] = []
    for coordinator, mac in _targets(call.hass, call):
        body = dict(payload)
        opts = coordinator.config_entry.options
        body.setdefault("mode", opts.get(CONF_MODE, DEFAULT_MODE))
        if mac:
            body["mac"] = mac
        answer = await run_command(coordinator, coordinator.api.show(body))
        results.append(answer)
    return {"results": results}


async def _show_text(call: ServiceCall) -> ServiceResponse:
    entries = _loaded(call.hass)
    opts = entries[0].options if entries else {}
    payload: dict[str, Any] = {
        "text": call.data["text"],
        "color": color_value(call.data.get("color", opts.get(CONF_COLOR, DEFAULT_COLOR))),
        "background": color_value(call.data.get("background", opts.get(CONF_BACKGROUND, DEFAULT_BACKGROUND))),
    }
    for key in ("font_size", "show_seconds", "mode", "name"):
        if key in call.data:
            payload[key] = call.data[key]
    return await _show(call, payload)


async def _show_image(call: ServiceCall) -> ServiceResponse:
    data = await image_bytes(call.hass, call.data)
    payload: dict[str, Any] = {"data": b64(data)}
    for key in ("show_seconds", "mode", "name"):
        if key in call.data:
            payload[key] = call.data[key]
    if any(k in call.data for k in ("scale", "x_offset", "y_offset", "mirror")):
        payload["placement"] = {
            "scale": call.data.get("scale", 1.0),
            "x": call.data.get("x_offset", 0.0),
            "y": call.data.get("y_offset", 0.0),
            "mirror": call.data.get("mirror", False),
        }
    return await _show(call, payload)


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
    hass.services.async_register(DOMAIN, "play_file", _play_file, schema=FILE_SCHEMA)
    hass.services.async_register(DOMAIN, "delete_file", _delete_file, schema=FILE_SCHEMA)
