"""Pages that take turns on the fans.

Every page is a sub-entry of the server: a text (a template, so a price or a temperature is
filled in), with an optional icon, style and value (colour by value, or a meter ring). It shows
for its own number of seconds while its condition holds, cast live so nothing is stored on the
fan. While it shows, a changed value is shown at once. Pages have the lowest priority: any
message interrupts them, and a page that comes round during a message is skipped.
"""

from __future__ import annotations

import asyncio
from collections.abc import Callable
from contextlib import suppress
from typing import Any

from homeassistant.config_entries import ConfigSubentry
from homeassistant.core import HomeAssistant, callback
from homeassistant.exceptions import TemplateError
from homeassistant.helpers.event import TrackTemplate, async_track_template_result
from homeassistant.helpers.template import Template, result_as_boolean

from .api import P56Error
from .const import (
    CONF_BACKGROUND,
    CONF_COLOR,
    DEFAULT_BACKGROUND,
    DEFAULT_COLOR,
    DOMAIN,
    LOGGER,
    PAGE_DEFAULT_SECONDS,
    PAGE_NAME,
    PAGE_PRIORITY,
    SUBENTRY_PAGE,
)
from .show import color_value

# Nothing to show: look again after this many seconds.
IDLE_WAIT = 10
# A page's cast runs this much longer than its turn, so the next page follows without a gap.
OVERLAP = 2


class P56Pages:
    """The pages of one server and the loop that shows them in turn."""

    def __init__(self, hass: HomeAssistant, coordinator: Any) -> None:
        self.hass = hass
        self.coordinator = coordinator
        self.current: str | None = None
        self._task: asyncio.Task | None = None
        self._listeners: list[Callable[[], None]] = []

    @property
    def running(self) -> bool:
        return self._task is not None and not self._task.done()

    def pages(self) -> list[ConfigSubentry]:
        """The pages in their order (then by name)."""
        subs = [s for s in self.coordinator.config_entry.subentries.values() if s.subentry_type == SUBENTRY_PAGE]
        return sorted(subs, key=lambda s: (float(s.data.get("order") or 0), s.title.lower()))

    @callback
    def add_listener(self, listener: Callable[[], None]) -> Callable[[], None]:
        """Called when the loop starts or stops, or another page comes up."""
        self._listeners.append(listener)
        return lambda: self._listeners.remove(listener)

    @callback
    def _changed(self) -> None:
        for listener in list(self._listeners):
            listener()

    @callback
    def start(self) -> None:
        if self.running:
            return
        entry = self.coordinator.config_entry
        self._task = entry.async_create_background_task(self.hass, self._run(), f"{DOMAIN} pages {entry.entry_id}")
        self._changed()

    @callback
    def stop(self) -> None:
        """Stop the loop; the page on the fan stops too."""
        task, self._task = self._task, None
        if task is not None:
            task.cancel()
        if self.current is not None:
            self.current = None
            self.hass.async_create_task(self._cast_stop())
        self._changed()

    async def _cast_stop(self) -> None:
        with suppress(P56Error):
            await self.coordinator.api.call("cast_stop", {})

    async def _run(self) -> None:
        while True:
            shown = False
            for page in self.pages():
                if self._enabled(page):
                    shown = True
                    await self._show_for(page)
            if not shown:
                if self.current is not None:
                    self.current = None
                    self._changed()
                await asyncio.sleep(IDLE_WAIT)

    def _enabled(self, page: ConfigSubentry) -> bool:
        condition = (page.data.get("show_if") or "").strip()
        if not condition:
            return True
        try:
            return result_as_boolean(Template(condition, self.hass).async_render(parse_result=False))
        except TemplateError as err:
            LOGGER.warning("page %s: condition cannot be read: %s", page.title, err)
            return False

    async def _show_for(self, page: ConfigSubentry) -> None:
        """Show one page for its time; a changed value is shown at once."""
        seconds = float(page.data.get("seconds") or PAGE_DEFAULT_SECONDS)
        self.current = page.title
        self._changed()
        until = self.hass.loop.time() + seconds
        await self._send(page, seconds)
        templates = [Template(t, self.hass) for t in (page.data.get("text"), page.data.get("value")) if t and "{" in t]
        unsubscribe: Callable[[], None] | None = None
        if templates:

            @callback
            def _updated(_event: Any, _updates: Any) -> None:
                left = until - self.hass.loop.time()
                if left > 1:
                    self.hass.async_create_task(self._send(page, left))

            info = async_track_template_result(self.hass, [TrackTemplate(t, None) for t in templates], _updated)
            unsubscribe = info.async_remove
        try:
            await asyncio.sleep(seconds)
        finally:
            if unsubscribe is not None:
                unsubscribe()

    def _render(self, text: str) -> str:
        if "{" not in text:
            return text
        try:
            return str(Template(text, self.hass).async_render(parse_result=False))
        except TemplateError as err:
            LOGGER.warning("page template cannot be read: %s", err)
            return text

    def payload(self, page: ConfigSubentry, seconds: float) -> dict[str, Any]:
        """What the server gets for a page: cast live, lowest priority, skipped during a message."""
        d = page.data
        body: dict[str, Any] = {
            "text": self._render(d.get("text") or ""),
            "name": PAGE_NAME,
            "mode": "cast",
            "show_seconds": round(seconds + OVERLAP, 1),
            "priority": PAGE_PRIORITY,
            "queue": False,
        }
        for key in ("icon", "style"):
            if d.get(key):
                body[key] = d[key]
        opts = self.coordinator.config_entry.options
        if d.get("color"):
            body["color"] = color_value(d["color"])
        elif not d.get("style"):
            body["color"] = color_value(opts.get(CONF_COLOR, DEFAULT_COLOR))
        if not d.get("style"):
            body["background"] = color_value(opts.get(CONF_BACKGROUND, DEFAULT_BACKGROUND))
        if d.get("value") and d.get("value_min") is not None and d.get("value_max") is not None:
            try:
                value = float(self._render(str(d["value"])))
            except ValueError:
                value = None
            if value is not None:
                body["color_scale"] = {
                    "value": value,
                    "min": d["value_min"],
                    "max": d["value_max"],
                    "invert": bool(d.get("invert_colors")),
                }
                if d.get("gauge"):
                    body["gauge"] = True
        return body

    async def _send(self, page: ConfigSubentry, seconds: float) -> None:
        try:
            await self.coordinator.api.show(self.payload(page, seconds))
        except P56Error as err:
            LOGGER.debug("page %s not shown: %s", page.title, err)
