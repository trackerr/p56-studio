"""Picture rotation (angle) and volume."""

from __future__ import annotations

from homeassistant.components.number import NumberEntity, NumberMode, RestoreNumber
from homeassistant.const import DEGREE, EntityCategory
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback

from .const import ANGLE_MAX, ANGLE_MIN
from .coordinator import P56ConfigEntry
from .entity import P56FanEntity, add_fan_entities

PARALLEL_UPDATES = 1


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    add_fan_entities(
        coordinator, async_add_entities, lambda mac: [P56Angle(coordinator, mac), P56Volume(coordinator, mac)]
    )


class P56Angle(P56FanEntity, NumberEntity):
    _attr_entity_category = EntityCategory.CONFIG
    _attr_native_min_value = ANGLE_MIN
    _attr_native_max_value = ANGLE_MAX
    _attr_native_step = 1
    _attr_native_unit_of_measurement = DEGREE
    _attr_mode = NumberMode.SLIDER

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "angle")

    @property
    def native_value(self) -> float | None:
        return self.status.get("angle")

    async def async_set_native_value(self, value: float) -> None:
        await self._set("angle", int(value))


class P56Volume(P56FanEntity, RestoreNumber):
    """The fan does not report its volume: remember what was set last."""

    _attr_entity_category = EntityCategory.CONFIG
    _attr_entity_registry_enabled_default = False
    _attr_native_min_value = 0
    _attr_native_max_value = 10
    _attr_native_step = 1
    _attr_mode = NumberMode.SLIDER
    _attr_assumed_state = True

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "volume")
        self._attr_native_value = None

    async def async_added_to_hass(self) -> None:
        await super().async_added_to_hass()
        if (last := await self.async_get_last_number_data()) is not None:
            self._attr_native_value = last.native_value

    async def async_set_native_value(self, value: float) -> None:
        await self._set("volume", int(value))
        self._attr_native_value = value
        self.async_write_ha_state()
