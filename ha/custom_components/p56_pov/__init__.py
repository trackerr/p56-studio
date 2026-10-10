"""P56 POV Display: control Holoscope/P56 POV fans through the P56 add-on (p56-server)."""

from __future__ import annotations

from homeassistant.const import CONF_HOST, CONF_PORT, Platform
from homeassistant.core import HomeAssistant
from homeassistant.helpers import config_validation as cv, device_registry as dr
from homeassistant.helpers.aiohttp_client import async_get_clientsession
from homeassistant.helpers.typing import ConfigType

from .api import P56Api
from .const import DEFAULT_HOST, DEFAULT_PORT, DOMAIN, LOGGER
from .coordinator import P56ConfigEntry, P56Coordinator
from .entity import server_device_info
from .services import async_setup_services

PLATFORMS: list[Platform] = [
    Platform.BUTTON,
    Platform.IMAGE,
    Platform.LIGHT,
    Platform.NOTIFY,
    Platform.NUMBER,
    Platform.SELECT,
    Platform.SENSOR,
    Platform.SWITCH,
    Platform.TEXT,
]

CONFIG_SCHEMA = cv.config_entry_only_config_schema(DOMAIN)


async def async_setup(hass: HomeAssistant, config: ConfigType) -> bool:
    async_setup_services(hass)
    return True


async def async_setup_entry(hass: HomeAssistant, entry: P56ConfigEntry) -> bool:
    api = P56Api(
        async_get_clientsession(hass),
        entry.data.get(CONF_HOST, DEFAULT_HOST),
        entry.data.get(CONF_PORT, DEFAULT_PORT),
    )
    coordinator = P56Coordinator(hass, entry, api)
    await coordinator.async_config_entry_first_refresh()
    entry.runtime_data = coordinator
    server = dr.async_get(hass).async_get_or_create(config_entry_id=entry.entry_id, **server_device_info(coordinator))
    coordinator.server_device_id = server.id
    entry.async_create_background_task(hass, coordinator.async_listen(), f"{DOMAIN} events {api.base}")
    await hass.config_entries.async_forward_entry_setups(entry, PLATFORMS)
    entry.async_on_unload(entry.add_update_listener(_options_updated))
    return True


async def _options_updated(hass: HomeAssistant, entry: P56ConfigEntry) -> None:
    await hass.config_entries.async_reload(entry.entry_id)


async def async_unload_entry(hass: HomeAssistant, entry: P56ConfigEntry) -> bool:
    return await hass.config_entries.async_unload_platforms(entry, PLATFORMS)


async def async_migrate_entry(hass: HomeAssistant, entry: P56ConfigEntry) -> bool:
    """1.x entries pointed at the old add-on; keep host and port."""
    if entry.version > 2:
        return False
    if entry.version == 1:
        data = {
            CONF_HOST: entry.data.get(CONF_HOST, DEFAULT_HOST),
            CONF_PORT: int(entry.data.get(CONF_PORT, DEFAULT_PORT)),
        }
        hass.config_entries.async_update_entry(
            entry, data=data, version=2, minor_version=1, unique_id=f"{data[CONF_HOST]}:{data[CONF_PORT]}"
        )
        LOGGER.info("P56 entry migrated to version 2")
    return True
