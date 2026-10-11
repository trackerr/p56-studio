"""Turn Home Assistant sources (file, URL, camera, media library) into a picture for the fan."""

from __future__ import annotations

import asyncio
import base64
import os
import shutil
import tempfile
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


def is_video(data: bytes) -> bool:
    """MP4/MOV, WebM/MKV or AVI, by their file signature."""
    head = data[:16]
    return head[4:8] == b"ftyp" or head[:4] == b"\x1a\x45\xdf\xa3" or (head[:4] == b"RIFF" and head[8:12] == b"AVI ")


def _ffmpeg_binary(hass: HomeAssistant) -> str | None:
    try:
        from homeassistant.components.ffmpeg import get_ffmpeg_manager  # noqa: PLC0415

        return get_ffmpeg_manager(hass).binary
    except Exception:  # noqa: BLE001 - ffmpeg integration not set up: try the PATH
        return shutil.which("ffmpeg")


def split_jpegs(stream: bytes) -> list[bytes]:
    """Split a concatenated MJPEG stream (ffmpeg image2pipe) into separate JPEGs."""
    frames, pos = [], 0
    while (start := stream.find(b"\xff\xd8", pos)) != -1:
        end = stream.find(b"\xff\xd9", start + 2)
        if end == -1:
            break
        frames.append(stream[start : end + 2])
        pos = end + 2
    return frames


async def video_frames(hass: HomeAssistant, data: bytes, fps: float, max_frames: int) -> list[bytes]:
    """Frames of a video as JPEGs (720 px), with the ffmpeg that comes with Home Assistant."""
    binary = _ffmpeg_binary(hass)
    if not binary:
        raise HomeAssistantError(translation_domain=DOMAIN, translation_key="video_needs_ffmpeg")

    def _write() -> str:
        with tempfile.NamedTemporaryFile(prefix="p56_", suffix=".video", delete=False) as f:
            f.write(data)
            return f.name

    path = await hass.async_add_executor_job(_write)
    try:
        proc = await asyncio.create_subprocess_exec(
            binary, "-hide_banner", "-loglevel", "error", "-i", path,
            "-vf", f"fps={fps},scale=720:720:force_original_aspect_ratio=decrease",
            "-frames:v", str(max_frames), "-f", "image2pipe", "-c:v", "mjpeg", "-q:v", "4", "pipe:1",
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        try:
            out, err = await asyncio.wait_for(proc.communicate(), timeout=120)
        except TimeoutError:
            proc.kill()
            raise HomeAssistantError(
                translation_domain=DOMAIN, translation_key="video_failed", translation_placeholders={"error": "timeout"}
            ) from None
    finally:
        await hass.async_add_executor_job(os.remove, path)
    frames = split_jpegs(out)
    if not frames:
        raise HomeAssistantError(
            translation_domain=DOMAIN,
            translation_key="video_failed",
            translation_placeholders={"error": err.decode(errors="replace").strip()[:200] or "no frames"},
        )
    return frames
