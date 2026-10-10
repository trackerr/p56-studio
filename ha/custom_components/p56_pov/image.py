"""Image entity: a round preview of what Home Assistant last sent to the fan."""

from __future__ import annotations

from datetime import datetime

from homeassistant.components.image import ImageEntity
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback

from .coordinator import P56ConfigEntry, P56Coordinator
from .entity import P56FanEntity, add_fan_entities

PARALLEL_UPDATES = 0


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    add_fan_entities(coordinator, async_add_entities, lambda mac: [P56Shown(hass, coordinator, mac)])


class P56Shown(P56FanEntity, ImageEntity):
    _attr_content_type = "image/png"

    def __init__(self, hass: HomeAssistant, coordinator: P56Coordinator, mac: str) -> None:
        P56FanEntity.__init__(self, coordinator, mac, "shown")
        ImageEntity.__init__(self, hass)

    @property
    def available(self) -> bool:
        return self.coordinator.last_update_success

    @property
    def image_last_updated(self) -> datetime | None:
        return self.coordinator.shown_at.get(self.mac)

    async def async_image(self) -> bytes | None:
        return await self.coordinator.api.shown(self.mac)
