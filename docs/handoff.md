# Smart Alarm Clock — Handoff

Context for continuing this project: the locked design, key reasoning, what's built, and what's open.

## In one paragraph
A custom-hardware, embedded-Rust, Home-Assistant-aware bedside **smart alarm clock**. Aesthetic:
**"dark & silent until summoned"** — a wood-veneer bar that shows nothing until a touch reveals the
time glowing *through* the veneer. Integrates with Home Assistant, but fires alarms **on-device** so
it works with WiFi/HA down.

---

## Locked decisions

### Architecture & firmware
- HA-aware but **offline-capable**; alarm firing lives on-device; the device is the **source of truth**.
- **Rust, `std` path** (`esp-idf-hal` + `esp-idf-svc`), FreeRTOS via ESP-IDF exposed as `std::thread`.
  Chosen for the mature WiFi/TLS/MQTT stack (`no_std` WiFi would be a fight).

### MCU & power
- **ESP32-S3-WROOM-1** (native USB-JTAG for program+debug). Bench board: **YD-ESP32-S3** devkit.
- **Time:** SNTP primary + **DS3231 RTC** + **supercap** backup (no battery) — RTC gives correct time
  on boot / when offline; SNTP corrects drift.
- **Power:** USB-C mains only, 5V→3.3V buck. No device battery. Accepted limit: a power cut spanning
  the alarm time misses it (phone is the backstop); the supercap keeps time so there's no blinking 12:00.

### Display
- Monochrome **dot-matrix showing time only**, behind warm wood veneer, **OFF when idle** (true dark).
  Bench: **red MAX7219 32×8** (validates the `HH:MM` layout + glow); final: a **warm/amber** emitter (PCB-stage).
- **Status (armed / ringing / AP / syncing) via a single RGB LED** — already driven on the onboard
  WS2812 with phase colors, so it costs no display space.

### Interaction — native capacitive touch
- **ESP32-S3 native capacitive touch** on a copper/foil electrode behind the veneer. Grammar:
  **tap = reveal/snooze, ~2.5 s hold = dismiss**. Rear buttons available.
- *Why:* capacitive senses **through wood**, which removes the biggest PCB-blocking unknown
  (IR-through-veneer). *(Pivot 2026-07-25 from a VCNL4040 proximity sensor.)* Fallback if touch feels
  wrong: a GY-APDS-9900 IR-reflective proximity module (reintroduces the IR test).

### Audio
- v1: **passive buzzer** via LEDC/PWM on GPIO4, switched by a **BS170** low-side MOSFET off 5 V.
- Future: real audio — default tone on device + user-uploadable files + registering as an HA
  `media_player` — needs a PSRAM board (N16R8) + MAX98357A + speaker. Open: WAV vs MP3, Rust vs
  ESPHome. See [Hardware → Future option](../hardware/README.md#future-option--real-audio--ha-media-player).

### Alarm model
- **Fixed pool of 8 slots** (`NUM_PRESETS` in `../software/firmware/src/state.rs`), each a time-of-day
  + `enabled`. (Repeat-days / sound / sunrise are future per-slot fields.)
- **Implicit arming:** `Armed` whenever any slot is enabled, else `Idle` — no arm/disarm control.
- **Dismiss is one-shot:** dismissing disables the slot that fired. Snooze re-rings after the interval.

### Config & HA integration
- **Setup:** first boot / WiFi change / recovery → WiFi **AP + captive portal** (self-contained).
  Everyday: **mDNS `.local`** web UI. Settings persisted in **NVS**.
- **HA — two paths, both wired:** (1) **MQTT discovery + LWT**; (2) a **custom integration** (REST +
  **SSE** realtime, **ports 80/81**, mDNS discovery, no broker required). Dashboards / automations /
  theme live in the `home-lab` repo; this repo owns only the integration.
- **Source-of-truth rule:** the device owns state; local actions and HA commands are both inputs to
  one on-device state machine; offline → local control unaffected, resync on reconnect.

### PCB & enclosure
- 2-layer KiCad, 0805 passives, module's onboard antenna, hand-soldered v1. Fab TBD (JLCPCB vs Aisler EU).
- Bedside landscape **wood-veneer bar**; matrix glows through the veneer; concealed USB-C; hidden fasteners.

---

## Firmware (current)
Real firmware + a working HA integration exist (past the scaffold stage). Code: `../software/firmware/`.
Worker threads (each a `std::thread`; the alarm core never blocks on the network):

- **alarm** — source of truth: 8-slot model + state machine + command bus; SNTP wall-clock firing.
- **net** — SoftAP + captive portal, HTTP REST API + web UI, SSE push (:81), mDNS, MQTT discovery/LWT.
- **button** — BOOT button → commands.
- **buzzer** — passive buzzer via LEDC (beeps while ringing).
- **led** — onboard WS2812 phase colors (status LED).

Every input transport (button, REST, MQTT) pushes the same `Command`s onto one bus.

**Not yet wired (bench tasks):** DS3231 RTC read (SNTP works; RTC boot-fallback scaffolded), MAX7219
time render, capacitive-touch input + tuning, NVS persistence of slots/settings.

**Toolchain:** `espup` (Xtensa S3 fork) → from `software/firmware/`, `cargo run` builds + flashes over USB-C.

---

## Bench validation (gates the PCB)
Parts, wiring, and rationale: [Hardware → Bench validation](../hardware/README.md#bench-validation).

1. **Bench-validate on a breadboard (~€30–55, ordered):** existing dev-kit + MAX7219 32×8 (time) +
   DS3231 + buzzer/BS170 + level converter + foil touch electrode + wood veneer samples. Prove the
   `HH:MM` glow, capacitive-touch-through-veneer, and the warm look before a PCB.
2. Flesh out firmware on the breadboard: RTC read, matrix render, touch input + tuning, NVS.
3. KiCad schematic → ERC → 2-layer layout → DRC → Gerbers → order.
4. Bring up the bare board incrementally; iterate the veneer enclosure.

**Status (2026-07-27):** parts arrived (veneer pending); buzzer firmware written; waiting on a
soldering iron to attach the dev-board header before breadboard bring-up.

---

## Open questions
- Warm display-emitter sourcing (bench uses red).
- Capacitive-touch-through-veneer tuning (electrode size, thresholds, groundless USB supply).
- Per-slot fields (repeat days / sound / sunrise) → drives the HA entity set + NVS schema.
- Enclosure dimensions; budget + fab (JLCPCB vs Aisler).

## v2 / stretch (leave hooks on v1)
Real audio / HA `media_player` (above); haptic dismiss (LRA + DRV2605L); sunrise light-ramp; BME280
climate (or the on-hand DS18B20); NFC preset tokens; broker-free HA (ESPHome-native / HomeKit).
