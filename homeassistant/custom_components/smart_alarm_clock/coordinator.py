"""Coordinator: slow polling fallback + realtime SSE push from the device."""

from __future__ import annotations

import asyncio
import json
import logging
from datetime import timedelta

import aiohttp

from homeassistant.core import HomeAssistant
from homeassistant.helpers.update_coordinator import DataUpdateCoordinator, UpdateFailed

from .api import SmartAlarmApi
from .const import DOMAIN

_LOGGER = logging.getLogger(__name__)

# The device pushes over SSE, so polling is only a slow safety net.
POLL_INTERVAL = timedelta(seconds=60)
SSE_PORT = 81


class SmartAlarmCoordinator(DataUpdateCoordinator[dict]):
    """Keeps device state; updated instantly by SSE, backed by polling."""

    def __init__(
        self, hass: HomeAssistant, api: SmartAlarmApi, session: aiohttp.ClientSession
    ) -> None:
        super().__init__(hass, _LOGGER, name=DOMAIN, update_interval=POLL_INTERVAL)
        self.api = api
        self._session = session
        self._sse_task: asyncio.Task | None = None

    async def _async_update_data(self) -> dict:
        try:
            return await self.api.get_state()
        except Exception as err:  # noqa: BLE001
            raise UpdateFailed(err) from err

    def start_push(self) -> None:
        """Start the background SSE listener (instant updates)."""
        if self._sse_task is None:
            self._sse_task = self.hass.async_create_background_task(
                self._sse_loop(), f"{DOMAIN}_sse"
            )

    def stop_push(self) -> None:
        if self._sse_task is not None:
            self._sse_task.cancel()
            self._sse_task = None

    async def _sse_loop(self) -> None:
        url = f"http://{self.api.host}:{SSE_PORT}/api/events"
        while True:
            try:
                timeout = aiohttp.ClientTimeout(total=None, sock_read=60)
                async with self._session.get(url, timeout=timeout) as resp:
                    async for raw in resp.content:
                        line = raw.decode(errors="replace").strip()
                        if line.startswith("data:"):
                            try:
                                self.async_set_updated_data(json.loads(line[5:].strip()))
                            except json.JSONDecodeError:
                                pass
            except asyncio.CancelledError:
                raise
            except Exception as err:  # noqa: BLE001
                _LOGGER.debug("SSE dropped (%s); reconnecting in 5s", err)
                await asyncio.sleep(5)
