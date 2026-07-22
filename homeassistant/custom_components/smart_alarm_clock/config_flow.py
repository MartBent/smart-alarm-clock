"""Config flow: manual host entry + Zeroconf auto-discovery."""

from __future__ import annotations

from typing import Any

import voluptuous as vol

from homeassistant.config_entries import ConfigFlow, ConfigFlowResult
from homeassistant.const import CONF_HOST
from homeassistant.helpers.aiohttp_client import async_get_clientsession
from homeassistant.helpers.service_info.zeroconf import ZeroconfServiceInfo

from .api import SmartAlarmApi
from .const import DEFAULT_NAME, DOMAIN


class SmartAlarmConfigFlow(ConfigFlow, domain=DOMAIN):
    """Handle the UI setup."""

    VERSION = 1

    def __init__(self) -> None:
        self._host: str | None = None

    async def async_step_user(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        errors: dict[str, str] = {}
        if user_input is not None:
            host = user_input[CONF_HOST]
            state = await self._fetch_state(host)
            if state is not None:
                await self._set_unique_id(host, state)
                return self.async_create_entry(title=DEFAULT_NAME, data={CONF_HOST: host})
            errors["base"] = "cannot_connect"
        return self.async_show_form(
            step_id="user",
            data_schema=vol.Schema({vol.Required(CONF_HOST): str}),
            errors=errors,
        )

    async def async_step_zeroconf(
        self, discovery_info: ZeroconfServiceInfo
    ) -> ConfigFlowResult:
        self._host = discovery_info.host
        state = await self._fetch_state(self._host)
        if state is None:
            return self.async_abort(reason="cannot_connect")
        # Sets the stable unique_id and, if this device is already configured,
        # aborts while refreshing CONF_HOST to the (possibly new) discovered IP.
        await self._set_unique_id(self._host, state)
        self.context["title_placeholders"] = {"name": DEFAULT_NAME}
        return await self.async_step_zeroconf_confirm()

    async def async_step_zeroconf_confirm(
        self, user_input: dict[str, Any] | None = None
    ) -> ConfigFlowResult:
        assert self._host is not None
        if user_input is not None:
            if await self._fetch_state(self._host) is not None:
                return self.async_create_entry(
                    title=DEFAULT_NAME, data={CONF_HOST: self._host}
                )
            return self.async_abort(reason="cannot_connect")
        return self.async_show_form(
            step_id="zeroconf_confirm",
            description_placeholders={"host": self._host},
        )

    async def _set_unique_id(self, host: str, state: dict) -> None:
        """Use the device's stable id as unique_id so the entry survives DHCP
        address changes; fall back to the host for older firmware without one."""
        unique_id = state.get("id") or host
        await self.async_set_unique_id(unique_id)
        self._abort_if_unique_id_configured(updates={CONF_HOST: host})

    async def _fetch_state(self, host: str) -> dict | None:
        """Return the device state (also serves as a reachability check), or None."""
        api = SmartAlarmApi(async_get_clientsession(self.hass), host)
        try:
            return await api.get_state()
        except Exception:  # noqa: BLE001
            return None
