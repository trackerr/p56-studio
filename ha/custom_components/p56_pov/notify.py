"""Notify entity: a message appears on the fan for a while, then the playlist resumes."""

from __future__ import annotations

from homeassistant.components.notify import NotifyEntity
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddConfigEntryEntitiesCallback

from .const import CONF_NOTIFY_SECONDS, DEFAULT_NOTIFY_SECONDS, NOTIFY_FILE_NAME
from .coordinator import P56ConfigEntry
from .entity import P56FanEntity, add_fan_entities, run_command
from .show import text_payload

PARALLEL_UPDATES = 1


async def async_setup_entry(
    hass: HomeAssistant, entry: P56ConfigEntry, async_add_entities: AddConfigEntryEntitiesCallback
) -> None:
    coordinator = entry.runtime_data
    add_fan_entities(coordinator, async_add_entities, lambda mac: [P56Notify(coordinator, mac)])


class P56Notify(P56FanEntity, NotifyEntity):
    def __init__(self, coordinator, mac: str) -> None:
        super().__init__(coordinator, mac, "message")

    async def async_send_message(self, message: str, title: str | None = None) -> None:
        text = f"{title}\n{message}" if title else message
        opts = self.coordinator.config_entry.options
        payload = text_payload(opts, text)
        payload.update(
            mac=self.mac,
            name=NOTIFY_FILE_NAME,
            show_seconds=opts.get(CONF_NOTIFY_SECONDS, DEFAULT_NOTIFY_SECONDS),
            mode="upload",
        )
        await run_command(self.coordinator, self.coordinator.api.show(payload))
