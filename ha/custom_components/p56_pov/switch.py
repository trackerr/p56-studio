"""Play/pause, the fan's on/off settings, and the pages of the server."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from homeassistant.components.switch import SwitchEntity, SwitchEntityDescription
from homeassistant.const import EntityCategory
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback
from homeassistant.helpers.restore_state import RestoreEntity

from .coordinator import P56ConfigEntry
from .entity import P56FanEntity, P56ServerEntity, add_fan_entities

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
    P56SwitchDescription(
        key="master", translation_key="master", field="master", on="master_on", off="master_off",
        entity_category=EntityCategory.CONFIG, entity_registry_enabled_default=False,
    ),
)


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    async_add_entities([P56PagesSwitch(coordinator)])
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


class P56PagesSwitch(P56ServerEntity, SwitchEntity, RestoreEntity):
    """The pages of this server take turns on the fans while this is on."""

    def __init__(self, coordinator) -> None:
        super().__init__(coordinator, "pages")

    async def async_added_to_hass(self) -> None:
        await super().async_added_to_hass()
        pages = self.coordinator.pages
        self.async_on_remove(pages.add_listener(self.async_write_ha_state))
        last = await self.async_get_last_state()
        if last is not None and last.state == "on":
            pages.start()

    @property
    def is_on(self) -> bool:
        return self.coordinator.pages.running

    @property
    def extra_state_attributes(self) -> dict[str, Any]:
        return {"pages": [p.title for p in self.coordinator.pages.pages()]}

    async def async_turn_on(self, **kwargs: Any) -> None:
        self.coordinator.pages.start()

    async def async_turn_off(self, **kwargs: Any) -> None:
        self.coordinator.pages.stop()
