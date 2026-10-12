"""Choose the file to play and the repeat mode."""

from __future__ import annotations

from homeassistant.components.select import SelectEntity
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
        lambda mac: [P56FileSelect(coordinator, mac), P56RepeatSelect(coordinator, mac)],
    )


class P56FileSelect(P56FanEntity, SelectEntity):
    """The playlist: pick what the fan plays."""

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "file")

    @property
    def options(self) -> list[str]:
        fan = self.fan
        return [name for _, name in fan.files] if fan else []

    @property
    def current_option(self) -> str | None:
        fan = self.fan
        return fan.current_file if fan else None

    async def async_select_option(self, option: str) -> None:
        fan = self._need_fan()
        slot = next((s for s, n in fan.files if n == option), None)
        if slot is not None:
            await self._set("select", slot)


class P56RepeatSelect(P56FanEntity, SelectEntity):
    _attr_options = ["all", "one"]

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "repeat")

    @property
    def current_option(self) -> str | None:
        value = self.status.get("repeat_all")
        return None if value is None else ("all" if value else "one")

    async def async_select_option(self, option: str) -> None:
        await self._control("repeat_all" if option == "all" else "repeat_one")
