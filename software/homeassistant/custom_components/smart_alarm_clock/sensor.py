"""Phase sensor."""

from __future__ import annotations

from homeassistant.components.sensor import SensorEntity
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
            PhaseSensor(coordinator, entry.entry_id),
            DeviceTimeSensor(coordinator, entry.entry_id),
        ]
    )


class PhaseSensor(SmartAlarmEntity, SensorEntity):
    _attr_name = "Phase"
    _attr_icon = "mdi:alarm"

    def __init__(self, coordinator, entry_id: str) -> None:
        super().__init__(coordinator, entry_id)
        self._attr_unique_id = f"{entry_id}_phase"

    @property
    def native_value(self) -> str | None:
        return (self.coordinator.data or {}).get("phase")


class DeviceTimeSensor(SmartAlarmEntity, SensorEntity):
    """The device's own wall clock (time-of-day it reports as `now`).

    Handy to confirm SNTP sync. It refreshes on each coordinator update — an SSE
    push on any material change, otherwise the 60s poll — so it tracks the
    device to the minute rather than ticking every second.
    """

    _attr_name = "Time"
    _attr_icon = "mdi:clock-outline"

    def __init__(self, coordinator, entry_id: str) -> None:
        super().__init__(coordinator, entry_id)
        self._attr_unique_id = f"{entry_id}_time"

    @property
    def native_value(self) -> str | None:
        return (self.coordinator.data or {}).get("now")
