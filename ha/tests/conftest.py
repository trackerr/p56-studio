"""Test fixtures: a real p56-server with its virtual fan (no hardware needed).

Set P56_SERVER to the p56-server binary; by default the debug build of this repository
(or of a p56-studio checkout next to it) is used.
"""

from __future__ import annotations

from collections.abc import Generator
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.request

import pytest
import pytest_socket

ROOT = Path(__file__).resolve().parents[1]


def _binary() -> Path:
    if env := os.environ.get("P56_SERVER"):
        return Path(env)
    exe = "p56-server.exe" if os.name == "nt" else "p56-server"
    for base in (ROOT.parent, ROOT.parent / "p56-studio", ROOT.parent / "p56"):
        cand = base / "target" / "debug" / exe
        if cand.exists():
            return cand
    pytest.skip("p56-server binary not found (set P56_SERVER)")


def _free(kind: int) -> int:
    with socket.socket(socket.AF_INET, kind) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


if os.name == "nt":
    # The asyncio loop on Windows needs a TCP socket pair; the tests only talk to 127.0.0.1.
    pytest_socket.disable_socket = lambda *args, **kwargs: None


@pytest.fixture(autouse=True)
def auto_enable_custom_integrations(enable_custom_integrations, socket_enabled):
    yield


@pytest.fixture(scope="session")
def p56_server() -> Generator[tuple[str, int]]:
    pytest_socket.enable_socket()
    http = _free(socket.SOCK_STREAM)
    args = [
        str(_binary()),
        "--http-port", str(http),
        "--tcp-port", str(_free(socket.SOCK_STREAM)),
        "--simulate",
        "--sim-udp-port", str(_free(socket.SOCK_DGRAM)),
    ]
    proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env={**os.environ, "RUST_LOG": "warn"})
    try:
        deadline = time.monotonic() + 20
        while True:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{http}/api/state", timeout=2) as r:
                    body = r.read()
                if b'"status":{' in body and b'"mac":""' not in body:
                    break
            except OSError:
                pass
            if time.monotonic() > deadline:
                raise RuntimeError("p56-server with virtual fan did not come up")
            time.sleep(0.2)
        yield ("127.0.0.1", http)
    finally:
        proc.kill()
        proc.wait()
