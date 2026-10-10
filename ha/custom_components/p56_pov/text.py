"""Text entity: type something (or set it from an automation) and the fan shows it."""

from __future__ import annotations

from homeassistant.components.text import TextEntity
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback
from homeassistant.helpers.restore_state import RestoreEntity

from .const import CONF_SHOW_SECONDS, DEFAULT_SHOW_SECONDS, TEXT_FILE_NAME
from .coordinator import P56ConfigEntry
from .entity import P56FanEntity, add_fan_entities, run_command
from .show import text_payload

PARALLEL_UPDATES = 1


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    add_fan_entities(coordinator, async_add_entities, lambda mac: [P56Text(coordinator, mac)])


class P56Text(P56FanEntity, TextEntity, RestoreEntity):
    _attr_native_max = 120

    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "text")
        self._attr_native_value = ""

    async def async_added_to_hass(self) -> None:
        await super().async_added_to_hass()
        if (last := await self.async_get_last_state()) is not None and last.state not in ("unknown", "unavailable"):
            self._attr_native_value = last.state

    async def async_set_value(self, value: str) -> None:
        opts = self.coordinator.config_entry.options
        if value.strip():
            payload = text_payload(opts, value)
            payload.update(
                mac=self.mac,
                name=TEXT_FILE_NAME,
                show_seconds=opts.get(CONF_SHOW_SECONDS, DEFAULT_SHOW_SECONDS),
            )
            await run_command(self.coordinator, self.coordinator.api.show(payload))
        else:
            fan = self._need_fan()
            slot = next((s for s, n in fan.files if n == f"{TEXT_FILE_NAME}.mp4"), None)
            if slot:
                await self._set("delete", slot)
        self._attr_native_value = value
        self.async_write_ha_state()
