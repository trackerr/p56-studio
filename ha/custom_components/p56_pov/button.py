"""Buttons: refresh and delete the file that is playing."""

from __future__ import annotations

from homeassistant.components.button import ButtonEntity
from homeassistant.const import EntityCategory
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback

from .coordinator import P56ConfigEntry
from .entity import P56FanEntity, add_fan_entities

PARALLEL_UPDATES = 1


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    add_fan_entities(
        coordinator,
        async_add_entities,
        lambda mac: [P56Refresh(coordinator, mac), P56DeleteCurrent(coordinator, mac)],
    )


class P56Refresh(P56FanEntity, ButtonEntity):
    _attr_entity_category = EntityCategory.DIAGNOSTIC

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "refresh")

    async def async_press(self) -> None:
        await self._control("refresh")


class P56DeleteCurrent(P56FanEntity, ButtonEntity):
    _attr_entity_registry_enabled_default = False

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "delete_current")

    async def async_press(self) -> None:
        slot = self.status.get("play_index")
        if slot:
            await self._set("delete", int(slot))
