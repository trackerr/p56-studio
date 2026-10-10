"""Constants for the P56 POV Display integration."""

from __future__ import annotations

import logging
from typing import Final

DOMAIN: Final = "p56_pov"
LOGGER = logging.getLogger(__package__)

DEFAULT_HOST: Final = "127.0.0.1"
DEFAULT_PORT: Final = 8765

# Options
CONF_SHOW_SECONDS: Final = "show_seconds"
CONF_NOTIFY_SECONDS: Final = "notify_seconds"
CONF_MODE: Final = "mode"
CONF_COLOR: Final = "color"
CONF_BACKGROUND: Final = "background"

DEFAULT_SHOW_SECONDS: Final = 0
DEFAULT_NOTIFY_SECONDS: Final = 15
DEFAULT_MODE: Final = "upload"
DEFAULT_COLOR: Final = "#ffffff"
DEFAULT_BACKGROUND: Final = "#000000"

MODES: Final = ["upload", "cast"]

# Name the text entity and notify use on the fan, so they replace their previous message.
TEXT_FILE_NAME: Final = "ha_tekst"
NOTIFY_FILE_NAME: Final = "ha_melding"

# Server events after which the state is fetched again.
REFRESH_EVENTS: Final = {
    "connected",
    "disconnected",
    "status",
    "info",
    "files",
    "error",
    "list",
    "done",
}

BRIGHTNESS_MAX: Final = 16
ANGLE_MIN: Final = -180
ANGLE_MAX: Final = 180
