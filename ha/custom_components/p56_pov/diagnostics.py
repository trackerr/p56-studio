"""Diagnostics download."""

from __future__ import annotations

from typing import Any

from homeassistant.components.diagnostics import async_redact_data
from homeassistant.core import HomeAssistant

from .coordinator import P56ConfigEntry

REDACT = {"mac", "hotspot", "router", "ip", "fan_ip", "host"}


async def async_get_config_entry_diagnostics(hass: HomeAssistant, entry: P56ConfigEntry) -> dict[str, Any]:
    coordinator = entry.runtime_data
    try:
        state = await coordinator.api.state()
    except Exception as err:  # noqa: BLE001
        state = {"error": str(err)}
    return {
        "entry": async_redact_data({"data": dict(entry.data), "options": dict(entry.options)}, REDACT),
        "live_events": coordinator.live,
        "last_update_success": coordinator.last_update_success,
        "server_state": async_redact_data(state, REDACT),
    }
