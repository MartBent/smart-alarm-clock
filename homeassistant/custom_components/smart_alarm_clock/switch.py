"""Armed switch + one enable switch per preset."""

from __future__ import annotations

from typing import Any

from homeassistant.components.switch import SwitchEntity
from homeassistant.config_entries import ConfigEntry
from homeassistant.core import HomeAssistant
from homeassistant.helpers.entity_platform import AddEntitiesCallback

from .const import ACTIVE_PHASES, DOMAIN
from .entity import SmartAlarmEntity


async def async_setup_entry(
    hass: HomeAssistant, entry: ConfigEntry, async_add_entities: AddEntitiesCallback
) -> None:
    coordinator = hass.data[DOMAIN][entry.entry_id]
    entities: list[SwitchEntity] = [ArmedSwitch(coordinator, entry.entry_id)]
    for preset in (coordinator.data or {}).get("presets", []):
        entities.append(
            PresetSwitch(coordinator, entry.entry_id, preset["idx"], preset["label"])
        )
    async_add_entities(entities)


class ArmedSwitch(SmartAlarmEntity, SwitchEntity):
    _attr_name = "Armed"
    _attr_icon = "mdi:alarm-check"

    def __init__(self, coordinator, entry_id: str) -> None:
        super().__init__(coordinator, entry_id)
        self._attr_unique_id = f"{entry_id}_armed"

    @property
    def is_on(self) -> bool:
        return (self.coordinator.data or {}).get("phase") in ACTIVE_PHASES

    async def async_turn_on(self, **kwargs: Any) -> None:
        await self.coordinator.api.command("arm")
        await self.coordinator.async_request_refresh()

    async def async_turn_off(self, **kwargs: Any) -> None:
        await self.coordinator.api.command("disarm")
        await self.coordinator.async_request_refresh()


class PresetSwitch(SmartAlarmEntity, SwitchEntity):
    _attr_icon = "mdi:alarm"

    def __init__(self, coordinator, entry_id: str, idx: int, label: str) -> None:
        super().__init__(coordinator, entry_id)
        self._idx = idx
        self._attr_name = f"Alarm {label}"
        self._attr_unique_id = f"{entry_id}_preset{idx}"

    def _preset(self) -> dict | None:
        for preset in (self.coordinator.data or {}).get("presets", []):
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
