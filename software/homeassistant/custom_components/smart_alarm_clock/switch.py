"""Switches: a Display master switch plus one enable switch per alarm slot.

There is no separate "Armed" switch: enabling any alarm arms the clock, and
dismissing a ringing alarm disables that slot (one-shot). The Display switch
turns all light-emitting components on/off (a firing alarm lights up anyway).
"""

from __future__ import annotations

from typing import Any

from homeassistant.components.switch import SwitchEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .const import DOMAIN
from .entity import SmartAlarmEntity


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    coordinator = hass.data[DOMAIN][entry.entry_id]
    entities: list[SwitchEntity] = [DisplaySwitch(coordinator, entry.entry_id)]
    # Fixed pool: one enable switch per slot the device reports at setup.
    entities += [
        PresetSwitch(coordinator, entry.entry_id, preset["idx"])
        for preset in (coordinator.data or {}).get("alarms", [])
    ]
    async_add_entities(entities)


class DisplaySwitch(SmartAlarmEntity, SwitchEntity):
    """Master on/off for all light-emitting components."""

    _attr_icon = "mdi:clock-digital"

    def __init__(self, coordinator, entry_id: str) -> None:
        super().__init__(coordinator, entry_id)
        self._attr_name = "Display"
        self._attr_unique_id = f"{entry_id}_display"

    @property
    def is_on(self) -> bool:
        return bool((self.coordinator.data or {}).get("display_on", True))

    async def async_turn_on(self, **kwargs: Any) -> None:
        await self.coordinator.api.set_display(True)
        await self.coordinator.async_request_refresh()

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self.coordinator.api.set_display(False)
        await self.coordinator.async_request_refresh()


class PresetSwitch(SmartAlarmEntity, SwitchEntity):
    _attr_icon = "mdi:alarm"

    def __init__(self, coordinator, entry_id: str, idx: int) -> None:
        super().__init__(coordinator, entry_id)
        self._idx = idx
        self._attr_name = f"Alarm {idx + 1}"
        self._attr_unique_id = f"{entry_id}_preset{idx}"

    def _preset(self) -> dict | None:
        for preset in (self.coordinator.data or {}).get("alarms", []):
            if preset["idx"] == self._idx:
                return preset
        return None

    @property
    def is_on(self) -> bool:
        preset = self._preset()
        return bool(preset and preset["enabled"])

    async def async_turn_on(self, **kwargs: Any) -> None:
        await self.coordinator.api.set_preset_enabled(self._idx, True)
        await self.coordinator.async_request_refresh()

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self.coordinator.api.set_preset_enabled(self._idx, False)
        await self.coordinator.async_request_refresh()
