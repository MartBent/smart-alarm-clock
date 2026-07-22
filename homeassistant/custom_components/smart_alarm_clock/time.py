"""Per-preset alarm time (editable from HA)."""

from __future__ import annotations

from datetime import time as dt_time

from homeassistant.components.time import TimeEntity
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
        PresetTime(coordinator, entry.entry_id, p["idx"], p["label"])
        for p in (coordinator.data or {}).get("presets", [])
    )


class PresetTime(SmartAlarmEntity, TimeEntity):
    _attr_icon = "mdi:clock-outline"

    def __init__(self, coordinator, entry_id: str, idx: int, label: str) -> None:
        super().__init__(coordinator, entry_id)
        self._idx = idx
        self._attr_name = f"{label} time"
        self._attr_unique_id = f"{entry_id}_preset{idx}_time"

    @property
    def native_value(self) -> dt_time | None:
        for preset in (self.coordinator.data or {}).get("presets", []):
            if preset["idx"] == self._idx:
                try:
                    h, m, s = (int(x) for x in preset["time"].split(":"))
                    return dt_time(h, m, s)
                except (ValueError, KeyError):
                    return None
        return None

    async def async_set_value(self, value: dt_time) -> None:
        await self.coordinator.api.set_preset_time(
            self._idx, value.hour, value.minute, value.second
        )
        await self.coordinator.async_request_refresh()
