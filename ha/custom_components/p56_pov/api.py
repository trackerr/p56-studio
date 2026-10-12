"""Small async client for the p56-server HTTP API (the P56 POV Display add-on)."""

from __future__ import annotations

import asyncio
from collections.abc import AsyncIterator
import json
from typing import Any

import aiohttp


class P56Error(Exception):
    """Base error."""


class P56ConnectionError(P56Error):
    """Server not reachable."""


class P56CommandError(P56Error):
    """The server (or the fan) refused a command."""


class P56OldServerError(P56Error):
    """An add-on 1.x server answered: it has no 2.x API."""


class P56Api:
    """Client for one p56-server."""

    def __init__(self, session: aiohttp.ClientSession, host: str, port: int) -> None:
        self._session = session
        # p56-server listens on IPv4; "localhost" may resolve to ::1 first.
        self.host = "127.0.0.1" if host.strip().lower() == "localhost" else host
        self.port = port

    @property
    def base(self) -> str:
        host = f"[{self.host}]" if ":" in self.host else self.host
        return f"http://{host}:{self.port}"

    async def _request(self, method: str, path: str, *, json_body: Any = None, timeout: float = 15) -> Any:
        try:
            async with self._session.request(
                method,
                f"{self.base}{path}",
                json=json_body,
                timeout=aiohttp.ClientTimeout(total=timeout),
            ) as resp:
                if resp.content_type == "application/json":
                    data = await resp.json()
                else:
                    data = await resp.read()
                if resp.status >= 500 or resp.status in (400, 404):
                    msg = data.get("error") if isinstance(data, dict) else None
                    raise P56CommandError(msg or f"HTTP {resp.status}")
                return data
        except (aiohttp.ClientError, asyncio.TimeoutError) as err:
            raise P56ConnectionError(f"{self.base}: {err}") from err

    async def ping(self) -> dict[str, Any]:
        """Check that a 2.x p56-server answers."""
        try:
            data = await self._request("GET", "/api/ping", timeout=8)
        except P56CommandError as err:
            try:
                await self._request("GET", "/health", timeout=8)
            except P56Error:
                raise err from None
            raise P56OldServerError("add-on 1.x") from err
        if not isinstance(data, dict) or data.get("server") != "p56-server":
            raise P56ConnectionError("not a p56-server")
        return data

    async def state(self) -> dict[str, Any]:
        data = await self._request("GET", "/api/state")
        if not isinstance(data, dict):
            raise P56ConnectionError("unexpected answer")
        return data

    async def call(self, cmd: str, args: dict[str, Any] | None = None, *, timeout: float = 30) -> Any:
        data = await self._request("POST", f"/api/call/{cmd}", json_body=args or {}, timeout=timeout)
        if not isinstance(data, dict) or not data.get("ok"):
            raise P56CommandError(data.get("error") if isinstance(data, dict) else "unexpected answer")
        return data.get("result")

    async def control(self, fan_id: int, action: str) -> None:
        await self.call("control", {"id": fan_id, "action": action})

    async def set_value(self, fan_id: int, what: str, value: int) -> None:
        await self.call("set_value", {"id": fan_id, "what": what, "value": value})

    async def show(self, payload: dict[str, Any]) -> dict[str, Any]:
        """Show text or a picture (see the add-on docs for the fields)."""
        return self._shown(await self._request("POST", "/api/show", json_body=payload, timeout=120))

    async def show_ai(self, payload: dict[str, Any]) -> dict[str, Any]:
        """A picture made by Home Assistant's AI Task, onto the fans (the add-on's server only)."""
        return self._shown(await self.call("show_ai", payload, timeout=180))

    @staticmethod
    def _shown(data: Any) -> dict[str, Any]:
        if not isinstance(data, dict):
            raise P56CommandError("unexpected answer")
        if not data.get("success"):
            errors = [r.get("error") for r in data.get("results", []) if r.get("error")]
            raise P56CommandError(data.get("error") or "; ".join(errors) or "failed")
        return data

    async def shown(self, key: str) -> bytes | None:
        """PNG of what the fan (by MAC) last got from the server, if any."""
        try:
            async with self._session.get(
                f"{self.base}/api/shown/{key}", timeout=aiohttp.ClientTimeout(total=10)
            ) as resp:
                if resp.status != 200:
                    return None
                return await resp.read()
        except (aiohttp.ClientError, asyncio.TimeoutError):
            return None

    async def events(self) -> AsyncIterator[dict[str, Any]]:
        """Live events (server-sent events). Ends when the connection drops."""
        try:
            async with self._session.get(
                f"{self.base}/api/events",
                timeout=aiohttp.ClientTimeout(total=None, sock_connect=10, sock_read=60),
                headers={"Accept": "text/event-stream"},
            ) as resp:
                if resp.status != 200:
                    raise P56ConnectionError(f"events: HTTP {resp.status}")
                yield {"type": "open"}
                async for raw in resp.content:
                    line = raw.decode(errors="replace").strip()
                    if not line.startswith("data:"):
                        continue
                    try:
                        yield json.loads(line[5:].strip())
                    except ValueError:
                        continue
        except (aiohttp.ClientError, asyncio.TimeoutError) as err:
            raise P56ConnectionError(f"events: {err}") from err
