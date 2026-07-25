"""The Smart Alarm Clock integration."""

from __future__ import annotations

from homeassistant.config_entries import ConfigEntry
from homeassistant.const import CONF_HOST, Platform
from homeassistant.core import HomeAssistant
from homeassistant.helpers.aiohttp_client import async_get_clientsession

from .api import SmartAlarmApi
from .const import DOMAIN
from .coordinator import SmartAlarmCoordinator

PLATFORMS = [Platform.SENSOR, Platform.SWITCH, Platform.BUTTON, Platform.TIME]


async def async_setup_entry(hass: HomeAssistant, entry: ConfigEntry) -> bool:
    session = async_get_clientsession(hass)
    api = SmartAlarmApi(session, entry.data[CONF_HOST])
    coordinator = SmartAlarmCoordinator(hass, api, session)
    await coordinator.async_config_entry_first_refresh()
    coordinator.start_push()

    hass.data.setdefault(DOMAIN, {})[entry.entry_id] = coordinator
    await hass.config_entries.async_forward_entry_setups(entry, PLATFORMS)
    return True


async def async_unload_entry(hass: HomeAssistant, entry: ConfigEntry) -> bool:
    coordinator: SmartAlarmCoordinator = hass.data[DOMAIN][entry.entry_id]
    coordinator.stop_push()
    unload_ok = await hass.config_entries.async_unload_platforms(entry, PLATFORMS)
    if unload_ok:
        hass.data[DOMAIN].pop(entry.entry_id)
    return unload_ok
