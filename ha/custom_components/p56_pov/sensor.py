"""Sensors: what plays, how many files, connection details."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime
from typing import Any

from homeassistant.components.sensor import SensorDeviceClass, SensorEntity, SensorEntityDescription, SensorStateClass
from homeassistant.const import EntityCategory
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback

from .coordinator import P56ConfigEntry, P56Coordinator, P56Fan
from .entity import P56FanEntity, P56ServerEntity, add_fan_entities

PARALLEL_UPDATES = 0


@dataclass(frozen=True, kw_only=True)
class P56FanSensorDescription(SensorEntityDescription):
    value: Callable[[P56Fan], Any]
    attrs: Callable[[P56Fan], dict[str, Any]] | None = None


FAN_SENSORS: tuple[P56FanSensorDescription, ...] = (
    P56FanSensorDescription(
        key="current_file",
        translation_key="current_file",
        value=lambda f: f.current_file,
        attrs=lambda f: {"slot": (f.status or {}).get("play_index")},
    ),
    P56FanSensorDescription(
        key="file_count",
        translation_key="file_count",
        state_class=SensorStateClass.MEASUREMENT,
        value=lambda f: len(f.files),
        attrs=lambda f: {"files": [{"slot": s, "name": n} for s, n in f.files]},
    ),
    P56FanSensorDescription(
        key="ip",
        translation_key="ip",
        entity_category=EntityCategory.DIAGNOSTIC,
        value=lambda f: f.ip.split(":")[0] if f.ip else None,
    ),
    P56FanSensorDescription(
        key="last_error",
        translation_key="last_error",
        entity_category=EntityCategory.DIAGNOSTIC,
        entity_registry_enabled_default=False,
        value=lambda f: f.last_error or "",
    ),
)


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    async_add_entities([P56FansConnected(coordinator), P56LastActivity(coordinator)])
    add_fan_entities(
        coordinator, async_add_entities, lambda mac: [P56FanSensor(coordinator, mac, d) for d in FAN_SENSORS]
    )


class P56FanSensor(P56FanEntity, SensorEntity):
    entity_description: P56FanSensorDescription

    def __init__(self, coordinator: P56Coordinator, mac: str, description: P56FanSensorDescription) -> None:
        super().__init__(coordinator, mac, description.key)
        self.entity_description = description

    @property
    def available(self) -> bool:
        return self.coordinator.last_update_success and self.fan is not None

    @property
    def native_value(self) -> Any:
        fan = self.fan
        return self.entity_description.value(fan) if fan else None

    @property
    def extra_state_attributes(self) -> dict[str, Any] | None:
        fan = self.fan
        if fan is None or self.entity_description.attrs is None:
            return None
        return self.entity_description.attrs(fan)


class P56FansConnected(P56ServerEntity, SensorEntity):
    """How many fans are connected to the server (0 = none)."""

    _attr_state_class = SensorStateClass.MEASUREMENT

    def __init__(self, coordinator: P56Coordinator) -> None:
        super().__init__(coordinator, "fans_connected")

    @property
    def native_value(self) -> int | None:
        return len(self.coordinator.data.fans) if self.coordinator.data else None

    @property
    def extra_state_attributes(self) -> dict[str, Any] | None:
        data = self.coordinator.data
        if data is None:
            return None
        return {
            "fans": [{"name": f.name, "mac": f.mac, "ip": f.ip} for f in data.fans.values()],
            "live_events": self.coordinator.live,
            "server_version": data.version,
        }


class P56LastActivity(P56ServerEntity, SensorEntity):
    _attr_device_class = SensorDeviceClass.TIMESTAMP
    _attr_entity_category = EntityCategory.DIAGNOSTIC

    def __init__(self, coordinator: P56Coordinator) -> None:
        super().__init__(coordinator, "last_activity")

    @property
    def native_value(self) -> datetime | None:
        return self.coordinator.data.last_activity if self.coordinator.data else None
