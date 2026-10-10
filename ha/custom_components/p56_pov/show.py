"""Turn Home Assistant sources (file, URL, camera, media library) into a picture for the fan."""

from __future__ import annotations

import base64
from collections.abc import Mapping
from pathlib import Path
from typing import Any
from urllib.parse import unquote

import aiohttp

from homeassistant.core import HomeAssistant
from homeassistant.exceptions import HomeAssistantError, ServiceValidationError
from homeassistant.helpers.aiohttp_client import async_get_clientsession

from .const import (
    CONF_BACKGROUND,
    CONF_COLOR,
    CONF_MODE,
    DEFAULT_BACKGROUND,
    DEFAULT_COLOR,
    DEFAULT_MODE,
    DOMAIN,
)

MAX_IMAGE_BYTES = 40 * 1024 * 1024


def text_payload(options: Mapping[str, Any], text: str) -> dict[str, Any]:
    """Text request with the entry's default colours and mode."""
    return {
        "text": text,
        "color": options.get(CONF_COLOR, DEFAULT_COLOR),
        "background": options.get(CONF_BACKGROUND, DEFAULT_BACKGROUND),
        "mode": options.get(CONF_MODE, DEFAULT_MODE),
    }


def color_value(value: Any) -> Any:
    """Colour from the UI (RGB list) or text (#rrggbb, name, "r,g,b")."""
    if isinstance(value, (list, tuple)) and len(value) == 3:
        return [int(v) for v in value]
    return value


def _invalid(key: str, **placeholders: str) -> ServiceValidationError:
    return ServiceValidationError(
        translation_domain=DOMAIN, translation_key=key, translation_placeholders=placeholders or None
    )


async def read_file(hass: HomeAssistant, path: str) -> bytes:
    if not hass.config.is_allowed_path(path):
        raise _invalid("path_not_allowed", path=path)

    def _read() -> bytes:
        p = Path(path)
        if not p.is_file():
            raise _invalid("file_not_found", path=path)
        if p.stat().st_size > MAX_IMAGE_BYTES:
            raise _invalid("file_too_large", path=path)
        return p.read_bytes()

    return await hass.async_add_executor_job(_read)


async def fetch_url(hass: HomeAssistant, url: str) -> bytes:
    session = async_get_clientsession(hass)
    try:
        async with session.get(url, timeout=aiohttp.ClientTimeout(total=30)) as resp:
            if resp.status != 200:
                raise HomeAssistantError(
                    translation_domain=DOMAIN,
                    translation_key="download_failed",
                    translation_placeholders={"url": url, "error": f"HTTP {resp.status}"},
                )
            data = await resp.content.read(MAX_IMAGE_BYTES + 1)
    except (aiohttp.ClientError, TimeoutError) as err:
        raise HomeAssistantError(
            translation_domain=DOMAIN,
            translation_key="download_failed",
            translation_placeholders={"url": url, "error": str(err)},
        ) from err
    if len(data) > MAX_IMAGE_BYTES:
        raise _invalid("file_too_large", path=url)
    return data


async def camera_image(hass: HomeAssistant, entity_id: str) -> bytes:
    from homeassistant.components.camera import async_get_image  # noqa: PLC0415

    image = await async_get_image(hass, entity_id)
    return image.content


async def media_image(hass: HomeAssistant, media: Mapping[str, Any] | str) -> bytes:
    """A file from the media library (media selector or media-source:// id)."""
    content_id = media if isinstance(media, str) else media.get("media_content_id", "")
    if not content_id:
        raise _invalid("no_source")
    # Local media folders: read straight from disk.
    prefix = "media-source://media_source/"
    if content_id.startswith(prefix):
        source_dir, _, rel = content_id[len(prefix):].partition("/")
        base = hass.config.media_dirs.get(source_dir)
        if base:
            full = (Path(base) / unquote(rel)).resolve()
            if Path(base).resolve() in full.parents:
                return await read_file(hass, str(full))
    from homeassistant.components import media_source  # noqa: PLC0415
    from homeassistant.components.media_player import async_process_play_media_url  # noqa: PLC0415

    try:
        resolved = await media_source.async_resolve_media(hass, content_id, None)
    except media_source.Unresolvable as err:
        raise _invalid("media_unresolvable", media=content_id) from err
    if (path := getattr(resolved, "path", None)) is not None:
        return await read_file(hass, str(path))
    return await fetch_url(hass, async_process_play_media_url(hass, resolved.url))


async def image_bytes(hass: HomeAssistant, data: Mapping[str, Any]) -> bytes:
    """The one source given in a show_image call."""
    sources = [k for k in ("path", "url", "camera", "media") if data.get(k)]
    if len(sources) != 1:
        raise _invalid("one_source")
    kind = sources[0]
    if kind == "path":
        return await read_file(hass, data["path"])
    if kind == "url":
        return await fetch_url(hass, data["url"])
    if kind == "camera":
        return await camera_image(hass, data["camera"])
    return await media_image(hass, data["media"])


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode()
