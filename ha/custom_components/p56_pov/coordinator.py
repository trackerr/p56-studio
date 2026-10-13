"""Coordinator: state from the add-on, kept fresh by its live event stream."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from typing import Any

from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant, callback
from homeassistant.helpers.debounce import Debouncer
from homeassistant.helpers.update_coordinator import DataUpdateCoordinator, UpdateFailed
from homeassistant.util import dt as dt_util

from .api import P56Api, P56Error
from .const import DOMAIN, LOGGER, REFRESH_EVENTS

POLL_LIVE = timedelta(seconds=60)
POLL_FALLBACK = timedelta(seconds=15)

type P56ConfigEntry = ConfigEntry[P56Coordinator]


@dataclass
class P56Fan:
    """One fan as the server reports it."""

    id: int
    mac: str
    name: str
    ip: str
    status: dict[str, Any] | None
    files: list[tuple[int, str]]
    last_error: str | None
    support: str
    raw: dict[str, Any] = field(repr=False)
    # the picture this server last sent: {"at": unix time, "live": still on the fan}
    shown: dict[str, Any] | None = None

    @property
    def current_file(self) -> str | None:
        if not self.status:
            return None
        slot = self.status.get("play_index")
        return next((n for s, n in self.files if s == slot), None)


@dataclass
class P56Data:
    """Snapshot of the server."""

    connected: bool
    fans: dict[str, P56Fan]
    last_activity: datetime | None
    version: str | None


def mac_key(mac: str) -> str:
    return mac.strip().lower()


def parse_state(state: dict[str, Any]) -> P56Data:
    fans: dict[str, P56Fan] = {}
    for dev in state.get("devices") or []:
        mac = mac_key(dev.get("mac") or "")
        if not mac:
            continue  # machine info not in yet: the fan shows up on the next update
        fans[mac] = P56Fan(
            id=int(dev["id"]),
            mac=mac,
            name=dev.get("name") or "P56",
            ip=dev.get("ip") or "",
            status=dev.get("status"),
            files=[(int(s), str(n)) for s, n in dev.get("files") or []],
            last_error=dev.get("last_error"),
            support=dev.get("support") or "",
            raw=dev,
            shown=dev.get("shown"),
        )
    last = state.get("last_activity")
    last_dt = None
    if isinstance(last, (int, float)) and last > 0:
        last_dt = dt_util.utc_from_timestamp(last)
    elif isinstance(last, str):
        last_dt = dt_util.parse_datetime(last)
    return P56Data(
        connected=bool(state.get("connected")),
        fans=fans,
        last_activity=last_dt,
        version=state.get("version"),
    )


class P56Coordinator(DataUpdateCoordinator[P56Data]):
    """Fetches `/api/state`; the event stream triggers a refresh at once."""

    config_entry: P56ConfigEntry

    def __init__(self, hass: HomeAssistant, entry: P56ConfigEntry, api: P56Api) -> None:
        super().__init__(
            hass,
            LOGGER,
            config_entry=entry,
            name=DOMAIN,
            update_interval=POLL_FALLBACK,
            request_refresh_debouncer=Debouncer(hass, LOGGER, cooldown=0.3, immediate=False),
        )
        self.api = api
        self.live = False
        self.server_device_id: str = ""

    async def _async_update_data(self) -> P56Data:
        try:
            return parse_state(await self.api.state())
        except P56Error as err:
            raise UpdateFailed(
                translation_domain=DOMAIN,
                translation_key="server_unreachable",
                translation_placeholders={"error": str(err)},
            ) from err

    def fan(self, mac: str) -> P56Fan | None:
        return self.data.fans.get(mac) if self.data else None

    async def async_listen(self) -> None:
        """Follow the server's event stream, reconnecting with back-off."""
        delay = 1.0
        while True:
            try:
                async for event in self.api.events():
                    if not self.live:
                        self.live = True
                        self.update_interval = POLL_LIVE
                        delay = 1.0
                        LOGGER.debug("live events from %s", self.api.base)
                    self._handle_event(event)
            except asyncio.CancelledError:
                raise
            except P56Error as err:
                LOGGER.debug("event stream: %s", err)
            except Exception:  # noqa: BLE001 - keep listening whatever happens
                LOGGER.exception("event stream failed")
            if self.live:
                # The add-on restarted or stopped: show that now, not at the next poll.
                self.live = False
                self.update_interval = POLL_FALLBACK
                await self.async_request_refresh()
            await asyncio.sleep(delay)
            delay = min(delay * 2, 10.0)

    @callback
    def _handle_event(self, event: dict[str, Any]) -> None:
        kind = event.get("type")
        if kind == "packet":
            kind = (event.get("data") or {}).get("kind")
        if kind in REFRESH_EVENTS or kind == "shown":
            self.hass.async_create_task(self.async_request_refresh(), eager_start=True)
