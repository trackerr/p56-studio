"""Image entity: a round preview of what Home Assistant last sent to the fan, faded once it is no
longer on the fan."""

from __future__ import annotations

from datetime import datetime
from typing import Any

from homeassistant.components.image import ImageEntity
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback
from homeassistant.util import dt as dt_util

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
        self._seen: tuple[Any, Any] | None = None
        self._follow()

    def _follow(self) -> None:
        """A new picture, or the same one going on or off the fan: the image changes."""
        fan = self.coordinator.fan(self.mac)
        shown = fan.shown if fan else None
        if not shown or not shown.get("at"):
            return
        seen = (shown.get("at"), shown.get("live"))
        if seen == self._seen:
            return
        # the first time (also after a restart) the moment it was sent; later the moment it changed
        self._attr_image_last_updated = (
            dt_util.utc_from_timestamp(shown["at"]) if self._seen is None else dt_util.utcnow()
        )
        self._seen = seen

    @callback
    def _handle_coordinator_update(self) -> None:
        self._follow()
        super()._handle_coordinator_update()

    @property
    def available(self) -> bool:
        return self.coordinator.last_update_success

    @property
    def extra_state_attributes(self) -> dict[str, Any]:
        return {"live": bool(self._seen and self._seen[1])}

    async def async_image(self) -> bytes | None:
        return await self.coordinator.api.shown(self.mac)
