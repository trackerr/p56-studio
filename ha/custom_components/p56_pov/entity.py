"""Base entities: one device per fan (by MAC) and one for the server."""

from __future__ import annotations

from collections.abc import Awaitable, Callable, Iterable
from typing import Any

from homeassistant.exceptions import HomeAssistantError
from homeassistant.core import callback
from homeassistant.helpers.device_registry import CONNECTION_NETWORK_MAC, DeviceEntryType, DeviceInfo
from homeassistant.helpers.entity import Entity
from homeassistant.helpers.entity_platform import AddEntitiesCallback
from homeassistant.helpers.update_coordinator import CoordinatorEntity

from .api import P56Error
from .const import DOMAIN
from .coordinator import P56Coordinator, P56Fan


def server_device_info(coordinator: P56Coordinator) -> DeviceInfo:
    entry = coordinator.config_entry
    slug = entry.data.get("addon_slug")
    return DeviceInfo(
        identifiers={(DOMAIN, entry.entry_id)},
        name="P56 server",
        manufacturer="p56-studio",
        model="p56-server",
        sw_version=coordinator.data.version if coordinator.data else None,
        entry_type=DeviceEntryType.SERVICE,
        configuration_url=f"homeassistant://hassio/ingress/{slug}" if slug else coordinator.api.base,
    )


async def run_command(coordinator: P56Coordinator, coro: Awaitable[Any]) -> Any:
    """Run a server command; turn failures into a readable Home Assistant error."""
    try:
        result = await coro
    except P56Error as err:
        raise HomeAssistantError(
            translation_domain=DOMAIN,
            translation_key="command_failed",
            translation_placeholders={"error": str(err)},
        ) from err
    await coordinator.async_request_refresh()
    return result


class P56ServerEntity(CoordinatorEntity[P56Coordinator]):
    """Entity of the server itself."""

    _attr_has_entity_name = True

    def __init__(self, coordinator: P56Coordinator, key: str) -> None:
        super().__init__(coordinator)
        self._attr_translation_key = key
        self._attr_unique_id = f"{coordinator.config_entry.entry_id}_{key}"
        self._attr_device_info = server_device_info(coordinator)


class P56FanEntity(CoordinatorEntity[P56Coordinator]):
    """Entity of one fan, found by MAC (fan ids change on every reconnect)."""

    _attr_has_entity_name = True

    def __init__(self, coordinator: P56Coordinator, mac: str, key: str) -> None:
        super().__init__(coordinator)
        self.mac = mac
        self._attr_translation_key = key
        self._attr_unique_id = f"{mac}_{key}"
        fan = coordinator.fan(mac)
        self._attr_device_info = DeviceInfo(
            identifiers={(DOMAIN, mac)},
            connections={(CONNECTION_NETWORK_MAC, mac)},
            name=fan.name if fan else "P56",
            manufacturer="Holoscope",
            model="POV fan",
            via_device_id=coordinator.server_device_id,
        )

    @property
    def fan(self) -> P56Fan | None:
        return self.coordinator.fan(self.mac)

    @property
    def status(self) -> dict[str, Any]:
        fan = self.fan
        return (fan.status or {}) if fan else {}

    @property
    def available(self) -> bool:
        fan = self.fan
        return super().available and fan is not None and fan.status is not None

    async def _control(self, action: str) -> None:
        fan = self._need_fan()
        await run_command(self.coordinator, self.coordinator.api.control(fan.id, action))

    async def _set(self, what: str, value: int) -> None:
        fan = self._need_fan()
        await run_command(self.coordinator, self.coordinator.api.set_value(fan.id, what, value))

    def _need_fan(self) -> P56Fan:
        fan = self.fan
        if fan is None:
            raise HomeAssistantError(translation_domain=DOMAIN, translation_key="fan_offline")
        return fan


def add_fan_entities(
    coordinator: P56Coordinator,
    async_add_entities: AddEntitiesCallback,
    factory: Callable[[str], Iterable[Entity]],
) -> None:
    """Add entities for every fan now and for each fan that connects later."""
    known: set[str] = set()

    @callback
    def check() -> None:
        if not coordinator.data:
            return
        new = [mac for mac in coordinator.data.fans if mac not in known]
        if not new:
            return
        known.update(new)
        async_add_entities([entity for mac in new for entity in factory(mac)])

    check()
    coordinator.config_entry.async_on_unload(coordinator.async_add_listener(check))
