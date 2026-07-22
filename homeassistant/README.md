# Home Assistant integration

A custom integration (`custom_components/smart_alarm_clock`) that adds the clock
to Home Assistant with **no MQTT broker** — it talks to the device's REST API
and gets instant updates over Server-Sent Events.

## Install

1. Copy `custom_components/smart_alarm_clock/` into your HA config directory:
   ```
   <config>/custom_components/smart_alarm_clock/
   ```
   (so you have `<config>/custom_components/smart_alarm_clock/manifest.json`).
2. Restart Home Assistant.

## Add the device

- **Auto-discovery**: once the device is on your network it advertises over
  mDNS, so HA shows **Settings → Devices & Services → "Smart Alarm Clock
  discovered"** → click **Configure**.
- **Manual**: Settings → Devices & Services → **Add Integration** → *Smart Alarm
  Clock* → enter the device's IP.

## Entities

One device, **Smart Alarm Clock**, with:

| Entity | Type | What it does |
| --- | --- | --- |
| Phase | sensor | `syncing` / `idle` / `armed` / `ringing` / `snoozed` |
| Armed | switch | arm / disarm |
| Snooze, Dismiss | button | while ringing |
| Alarm *&lt;name&gt;* | switch | enable/disable each preset |
| *&lt;name&gt;* time | time | edit each preset's time |

## How it works

- **Commands** → `POST /api/command`, `/api/preset/enabled`, `/api/preset/time`.
- **State** → `GET /api/state` (60 s polling fallback).
- **Realtime** → the device streams changes over SSE on `:81/api/events`, so
  entities update instantly (`iot_class: local_push`).

Requires a recent Home Assistant (uses `homeassistant.helpers.service_info.zeroconf`).
Give the device a DHCP reservation, or rely on mDNS, so its address is stable.
