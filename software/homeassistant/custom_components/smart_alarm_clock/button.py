"""Snooze / Dismiss buttons."""

from __future__ import annotations

from homeassistant.components.button import ButtonEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .const import DOMAIN
from .entity import SmartAlarmEntity


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    coordinator = hass.data[DOMAIN][entry.entry_id]
    async_add_entities(
        [
            CommandButton(coordinator, entry.entry_id, "Snooze", "snooze", "mdi:alarm-snooze"),
            CommandButton(coordinator, entry.entry_id, "Dismiss", "dismiss", "mdi:alarm-off"),
        ]
    )


class CommandButton(SmartAlarmEntity, ButtonEntity):
    def __init__(self, coordinator, entry_id: str, name: str, cmd: str, icon: str) -> None:
        super().__init__(coordinator, entry_id)
        self._cmd = cmd
        self._attr_name = name
        self._attr_icon = icon
        self._attr_unique_id = f"{entry_id}_{cmd}"

    async def async_press(self) -> None:
        await self.coordinator.api.command(self._cmd)
        await self.coordinator.async_request_refresh()
