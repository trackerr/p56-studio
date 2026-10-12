"""Config flow: found automatically through the add-on, or entered by hand."""

from __future__ import annotations

from typing import Any

import voluptuous as vol

from homeassistant.config_entries import (
    ConfigEntry,
    ConfigFlow,
    ConfigFlowResult,
    ConfigSubentryFlow,
    OptionsFlow,
    SubentryFlowResult,
)
from homeassistant.const import CONF_HOST, CONF_PORT
from homeassistant.core import callback
from homeassistant.helpers.aiohttp_client import async_get_clientsession
from homeassistant.helpers.selector import (
    BooleanSelector,
    IconSelector,
    NumberSelector,
    NumberSelectorConfig,
    NumberSelectorMode,
    SelectSelector,
    SelectSelectorConfig,
    TemplateSelector,
    TextSelector,
)
from homeassistant.helpers.service_info.hassio import HassioServiceInfo

from .api import P56Api, P56ConnectionError, P56Error, P56OldServerError
from .const import (
    CONF_BACKGROUND,
    CONF_COLOR,
    CONF_MODE,
    CONF_NOTIFY_SECONDS,
    CONF_SHOW_SECONDS,
    DEFAULT_BACKGROUND,
    DEFAULT_COLOR,
    DEFAULT_HOST,
    DEFAULT_MODE,
    DEFAULT_NOTIFY_SECONDS,
    DEFAULT_PORT,
    DEFAULT_SHOW_SECONDS,
    DOMAIN,
    LOGGER,
    MODES,
    PAGE_DEFAULT_SECONDS,
    STYLES,
    SUBENTRY_PAGE,
)


LOCAL_HOSTS = {"localhost", "127.0.0.1", "::1"}


def _schema(host: str = DEFAULT_HOST, port: int = DEFAULT_PORT) -> vol.Schema:
    return vol.Schema(
        {
            vol.Required(CONF_HOST, default=host): str,
            vol.Required(CONF_PORT, default=port): vol.All(vol.Coerce(int), vol.Range(min=1, max=65535)),
        }
    )


class P56ConfigFlow(ConfigFlow, domain=DOMAIN):
    VERSION = 2
    MINOR_VERSION = 1

    @classmethod
    @callback
    def async_get_supported_subentry_types(cls, config_entry: ConfigEntry) -> dict[str, type[ConfigSubentryFlow]]:
        return {SUBENTRY_PAGE: P56PageFlow}

    def __init__(self) -> None:
        self._discovered: dict[str, Any] = {}

    async def _check(self, host: str, port: int) -> str | None:
        try:
            await P56Api(async_get_clientsession(self.hass), host, port).ping()
        except P56OldServerError:
            return "old_server"
        except P56ConnectionError:
            return "cannot_connect"
        except P56Error:
            return "cannot_connect"
        except Exception:  # noqa: BLE001
            LOGGER.exception("unexpected error")
            return "unknown"
        return None

    async def async_step_user(self, user_input: dict[str, Any] | None = None) -> ConfigFlowResult:
        errors: dict[str, str] = {}
        if user_input is not None:
            host, port = user_input[CONF_HOST].strip(), user_input[CONF_PORT]
            await self.async_set_unique_id(f"{host}:{port}")
            self._abort_if_unique_id_configured()
            if (error := await self._check(host, port)) is None:
                return self.async_create_entry(title="P56 POV", data={CONF_HOST: host, CONF_PORT: port})
            errors["base"] = error
        return self.async_show_form(
            step_id="user",
            data_schema=self.add_suggested_values_to_schema(_schema(), user_input or {}),
            errors=errors,
        )

    async def async_step_hassio(self, discovery_info: HassioServiceInfo) -> ConfigFlowResult:
        """The add-on announced itself."""
        host = discovery_info.config.get("host", DEFAULT_HOST)
        port = int(discovery_info.config.get("port", DEFAULT_PORT))
        # An entry made by hand for this machine (localhost, 127.0.0.1) is the same server.
        for entry in self._async_current_entries(include_ignore=False):
            if entry.data.get(CONF_HOST) in LOCAL_HOSTS and int(entry.data.get(CONF_PORT, 0)) == port:
                if entry.data.get("addon_slug") != discovery_info.slug:
                    self.hass.config_entries.async_update_entry(
                        entry, data={**entry.data, "addon_slug": discovery_info.slug}
                    )
                return self.async_abort(reason="already_configured")
        await self.async_set_unique_id(f"{host}:{port}")
        self._abort_if_unique_id_configured(updates={CONF_HOST: host, CONF_PORT: port, "addon_slug": discovery_info.slug})
        self._discovered = {CONF_HOST: host, CONF_PORT: port, "addon_slug": discovery_info.slug, "name": discovery_info.name}
        return await self.async_step_hassio_confirm()

    async def async_step_hassio_confirm(self, user_input: dict[str, Any] | None = None) -> ConfigFlowResult:
        errors: dict[str, str] = {}
        d = self._discovered
        if user_input is not None:
            if (error := await self._check(d[CONF_HOST], d[CONF_PORT])) is None:
                return self.async_create_entry(
                    title="P56 POV",
                    data={CONF_HOST: d[CONF_HOST], CONF_PORT: d[CONF_PORT], "addon_slug": d["addon_slug"]},
                )
            errors["base"] = error
        return self.async_show_form(
            step_id="hassio_confirm", description_placeholders={"addon": d.get("name") or "P56"}, errors=errors
        )

    async def async_step_reconfigure(self, user_input: dict[str, Any] | None = None) -> ConfigFlowResult:
        entry = self._get_reconfigure_entry()
        errors: dict[str, str] = {}
        if user_input is not None:
            host, port = user_input[CONF_HOST].strip(), user_input[CONF_PORT]
            if (error := await self._check(host, port)) is None:
                return self.async_update_reload_and_abort(
                    entry, unique_id=f"{host}:{port}", data_updates={CONF_HOST: host, CONF_PORT: port}
                )
            errors["base"] = error
        return self.async_show_form(
            step_id="reconfigure",
            data_schema=_schema(entry.data.get(CONF_HOST, DEFAULT_HOST), entry.data.get(CONF_PORT, DEFAULT_PORT)),
            errors=errors,
        )

    @staticmethod
    @callback
    def async_get_options_flow(config_entry) -> OptionsFlow:
        return P56OptionsFlow()


class P56OptionsFlow(OptionsFlow):
    """Defaults for texts, notifications and the text entity."""

    async def async_step_init(self, user_input: dict[str, Any] | None = None) -> ConfigFlowResult:
        if user_input is not None:
            return self.async_create_entry(data=user_input)
        o = self.config_entry.options
        seconds = NumberSelector(NumberSelectorConfig(min=0, max=3600, step=1, mode=NumberSelectorMode.BOX, unit_of_measurement="s"))
        schema = vol.Schema(
            {
                vol.Required(CONF_MODE, default=o.get(CONF_MODE, DEFAULT_MODE)): SelectSelector(
                    SelectSelectorConfig(options=MODES, translation_key="mode")
                ),
                vol.Required(CONF_SHOW_SECONDS, default=o.get(CONF_SHOW_SECONDS, DEFAULT_SHOW_SECONDS)): seconds,
                vol.Required(CONF_NOTIFY_SECONDS, default=o.get(CONF_NOTIFY_SECONDS, DEFAULT_NOTIFY_SECONDS)): seconds,
                vol.Required(CONF_COLOR, default=o.get(CONF_COLOR, DEFAULT_COLOR)): TextSelector(),
                vol.Required(CONF_BACKGROUND, default=o.get(CONF_BACKGROUND, DEFAULT_BACKGROUND)): TextSelector(),
            }
        )
        return self.async_show_form(step_id="init", data_schema=schema)


def _number(minimum: float, maximum: float, step: float | str = 1, unit: str | None = None) -> NumberSelector:
    config = NumberSelectorConfig(min=minimum, max=maximum, step=step, mode=NumberSelectorMode.BOX)
    if unit:
        config["unit_of_measurement"] = unit
    return NumberSelector(config)


PAGE_SCHEMA = vol.Schema(
    {
        vol.Required("name"): TextSelector(),
        vol.Optional("text"): TemplateSelector(),
        vol.Optional("icon"): IconSelector(),
        vol.Optional("style"): SelectSelector(SelectSelectorConfig(options=STYLES, translation_key="style")),
        vol.Optional("color"): TextSelector(),
        vol.Optional("value"): TemplateSelector(),
        vol.Optional("value_min"): _number(-1_000_000, 1_000_000, "any"),
        vol.Optional("value_max"): _number(-1_000_000, 1_000_000, "any"),
        vol.Optional("invert_colors", default=False): BooleanSelector(),
        vol.Optional("gauge", default=False): BooleanSelector(),
        vol.Optional("show_if"): TemplateSelector(),
        vol.Required("seconds", default=PAGE_DEFAULT_SECONDS): _number(3, 3600, 1, "s"),
        vol.Optional("order", default=0): _number(0, 999),
    }
)


class P56PageFlow(ConfigSubentryFlow):
    """Add or change a page that takes turns on the fans."""

    async def async_step_user(self, user_input: dict[str, Any] | None = None) -> SubentryFlowResult:
        if user_input is not None:
            return self.async_create_entry(title=user_input["name"], data=user_input)
        return self.async_show_form(step_id="user", data_schema=PAGE_SCHEMA)

    async def async_step_reconfigure(self, user_input: dict[str, Any] | None = None) -> SubentryFlowResult:
        subentry = self._get_reconfigure_subentry()
        if user_input is not None:
            return self.async_update_and_abort(self._get_entry(), subentry, title=user_input["name"], data=user_input)
        return self.async_show_form(
            step_id="reconfigure", data_schema=self.add_suggested_values_to_schema(PAGE_SCHEMA, subentry.data)
        )
