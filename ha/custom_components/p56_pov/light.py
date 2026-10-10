"""Display power and brightness (1-16 on the fan)."""

from __future__ import annotations

from typing import Any

from homeassistant.components.light import ATTR_BRIGHTNESS, ColorMode, LightEntity
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback

from .const import BRIGHTNESS_MAX
from .coordinator import P56ConfigEntry
from .entity import P56FanEntity, add_fan_entities

PARALLEL_UPDATES = 1


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    add_fan_entities(coordinator, async_add_entities, lambda mac: [P56Light(coordinator, mac)])


def to_ha(level: int) -> int:
    return round(max(1, min(BRIGHTNESS_MAX, level)) * 255 / BRIGHTNESS_MAX)


def to_fan(brightness: int) -> int:
    return max(1, min(BRIGHTNESS_MAX, round(brightness * BRIGHTNESS_MAX / 255)))


class P56Light(P56FanEntity, LightEntity):
    """The picture: on/off and brightness."""

    _attr_name = None  # the device's own name
    _attr_color_mode = ColorMode.BRIGHTNESS
    _attr_supported_color_modes = {ColorMode.BRIGHTNESS}

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "display")

    @property
    def is_on(self) -> bool | None:
        power = self.status.get("power")
        return None if power is None else bool(power)

    @property
    def brightness(self) -> int | None:
        level = self.status.get("brightness")
        return None if level is None else to_ha(int(level))

    async def async_turn_on(self, **kwargs: Any) -> None:
        if not self.is_on:
            await self._control("on")
        if (b := kwargs.get(ATTR_BRIGHTNESS)) is not None:
            await self._set("brightness", to_fan(int(b)))

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self._control("off")
