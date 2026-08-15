# Home Assistant integration

The custom integration now lives in its own repository so it can be installed
through **HACS** (which requires `custom_components/` at the repo root):

### → [MartBent/homeassistant-smart-alarm-clock](https://github.com/MartBent/homeassistant-smart-alarm-clock)

It's **broker-free**: it talks to the device's local REST API and streams changes
over Server-Sent Events (mDNS discovery, ports 80/81). Install steps, the entity
list, and how it works are in that repo's README.

MQTT discovery + LWT is also wired from the firmware (`../firmware/src/mqtt.rs`)
as a second, broker-based path into Home Assistant.

> Develop/test without hardware using `../tools/sim_device.py`, which reproduces
> the device's REST API + SSE stream + state machine.
