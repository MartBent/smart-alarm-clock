"""Base entity: ties everything to one HA device and the coordinator."""

from __future__ import annotations

from homeassistant.helpers.device_registry import DeviceInfo
from homeassistant.helpers.update_coordinator import CoordinatorEntity

from .const import DEFAULT_NAME, DOMAIN
from .coordinator import SmartAlarmCoordinator


class SmartAlarmEntity(CoordinatorEntity[SmartAlarmCoordinator]):
    """Common base — has_entity_name + shared device_info."""

    _attr_has_entity_name = True

    def __init__(self, coordinator: SmartAlarmCoordinator, entry_id: str) -> None:
        super().__init__(coordinator)
        self._attr_device_info = DeviceInfo(
            identifiers={(DOMAIN, entry_id)},
            name=DEFAULT_NAME,
            manufacturer="DIY",
            model="v1",
        )
