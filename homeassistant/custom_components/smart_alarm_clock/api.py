"""Thin async client for the Smart Alarm Clock REST API."""

from __future__ import annotations

import asyncio

import aiohttp


class SmartAlarmApi:
    """Talks to the device's HTTP REST API (see firmware net.rs)."""

    def __init__(self, session: aiohttp.ClientSession, host: str) -> None:
        self._session = session
        self.host = host
        self._base = f"http://{host}"

    async def get_state(self) -> dict:
        """GET /api/state -> {phase, now, snooze_secs, presets:[...]}"""
        async with asyncio.timeout(10):
            async with self._session.get(f"{self._base}/api/state") as resp:
                resp.raise_for_status()
                return await resp.json()

    async def command(self, cmd: str) -> None:
        """POST /api/command {cmd: arm|disarm|snooze|dismiss}"""
        await self._post("/api/command", {"cmd": cmd})

    async def set_preset_enabled(self, idx: int, enabled: bool) -> None:
        await self._post("/api/preset/enabled", {"idx": idx, "enabled": enabled})

    async def set_preset_time(
        self, idx: int, hour: int, minute: int, second: int = 0
    ) -> None:
        await self._post(
            "/api/preset/time",
            {"idx": idx, "hour": hour, "minute": minute, "second": second},
        )

    async def _post(self, path: str, body: dict) -> None:
        async with asyncio.timeout(10):
            async with self._session.post(f"{self._base}{path}", json=body) as resp:
                resp.raise_for_status()
