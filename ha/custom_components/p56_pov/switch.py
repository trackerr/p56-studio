"""Play/pause and the fan's on/off settings."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from homeassistant.components.switch import SwitchEntity, SwitchEntityDescription
from homeassistant.const import EntityCategory
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback

from .coordinator import P56ConfigEntry
from .entity import P56FanEntity, add_fan_entities

PARALLEL_UPDATES = 1


@dataclass(frozen=True, kw_only=True)
class P56SwitchDescription(SwitchEntityDescription):
    field: str
    on: str
    off: str


SWITCHES: tuple[P56SwitchDescription, ...] = (
    P56SwitchDescription(key="playing", translation_key="playing", field="playing", on="play", off="pause"),
    P56SwitchDescription(
        key="breath", translation_key="breath", field="breath", on="breath_on", off="breath_off",
        entity_category=EntityCategory.CONFIG,
    ),
    P56SwitchDescription(
        key="autostart", translation_key="autostart", field="autostart", on="autostart_on", off="autostart_off",
        entity_category=EntityCategory.CONFIG,
    ),
)


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    add_fan_entities(
        coordinator, async_add_entities, lambda mac: [P56Switch(coordinator, mac, d) for d in SWITCHES]
    )


class P56Switch(P56FanEntity, SwitchEntity):
    entity_description: P56SwitchDescription

    def __init__(self, coordinator, mac: str, description: P56SwitchDescription) -> None:
        super().__init__(coordinator, mac, description.key)
        self.entity_description = description

    @property
    def is_on(self) -> bool | None:
        value = self.status.get(self.entity_description.field)
        return None if value is None else bool(value)

    async def async_turn_on(self, **kwargs: Any) -> None:
        await self._control(self.entity_description.on)

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self._control(self.entity_description.off)
