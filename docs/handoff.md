# Smart Alarm Clock — Project Handoff

A context document for continuing this project in a code session. It captures the locked
design, the reasoning behind key decisions, what's still open, and the immediate next steps.
A companion file (`smart-alarm-clock-design-options.md`) holds the full options-and-trade-offs
reference; this file is the executive summary + build-readiness notes.

---

## Project in one paragraph

A custom-hardware, custom-firmware, Home-Assistant-oriented **smart alarm clock**, built as a
hobby project with two explicit learning goals: **PCB design** (beginner) and **embedded Rust**
(experienced in embedded software generally). The defining aesthetic is **"dark & silent until
summoned"** — a minimal, non-technical bedside object (a landscape **wood-veneer bar**) that
shows nothing until a proximity gesture reveals the time through the veneer. It integrates fully
with Home Assistant but fires alarms on-device so it works even if WiFi/HA is down.

---

## Locked decisions

### Architecture & firmware
- **Architecture:** smart device, **HA-aware but offline-capable**. Alarm firing logic lives
  on-device; HA configures/observes and runs side automations. Device is the **source of truth**.
- **Language/framework:** **Rust, `std` path** — `esp-idf-hal` + `esp-idf-svc` (built on ESP-IDF).
- **Runtime model:** **RTOS** (FreeRTOS via ESP-IDF), exposed to Rust as `std::thread` / mutexes /
  channels.
- **Why `std` over `no_std`:** mature WiFi/lwIP/mbedTLS/MQTT stack is the deciding factor — the
  hard networking is solved on this path; `no_std`/esp-hal WiFi is younger and would be a fight.

### MCU & hardware
- **MCU:** **ESP32-S3-WROOM-1** (native USB-JTAG for program+debug; audio headroom for v2 I²S).
  Accepts the Xtensa Rust toolchain fork (`espup`).
- **Timekeeping:** **NTP (SNTP) primary** + **DS3231 RTC** + **supercapacitor** backup
  (no battery). Boot validity check: if RTC time valid → use immediately + NTP corrects drift;
  if invalid (week+ outage) → brief "syncing" state, wait for NTP, then set clock + RTC.
- **Power:** **USB-C mains only**, 5V → 3.3V **buck**. **No device battery** / charge / protection
  circuitry. Supercap backs the RTC only.
  - Accepted limitation: a power outage spanning the alarm time = that alarm missed (no power to
    fire). Supercap guarantees time is never lost (no blinking 12:00); phone is the backstop.
    Rechargeable-battery option was considered and rejected for v1 (charge/protection/power-path
    complexity + LiPo-on-nightstand safety). If revisited, do it as v2 with LiFePO4, or use an
    external UPS power bank (no board changes).
- **Display:** **diffused warm dot-matrix LED panel behind thin wood veneer**, monochrome,
  **driven off when idle** (true dark — no veneer trade-off for darkness). Shows current time +
  alarm time + option fields (dismiss-progress, "AP MODE", "ARMED", preset names).
  - LED type preference: **APA102 / SK9822 (SPI)** over WS2812 (far easier to drive reliably in
    Rust `std`; true per-pixel brightness for fades + dismiss-progress fill). Or a pre-made panel
    (its native driver dictates the crate).
- **Interaction sensor:** single **VCNL4040** (proximity + ambient light, I²C). Does the gesture
  AND the night-dimming lux — replaces a separate LDR. No presence sensor. No NFC.
- **Input — proximity gesture grammar:**
  - Idle → **quick reach reveals time** (fade up, hold ~5s, fade out).
  - Ringing → **quick reach = snooze**; **sustained hold ~2.5s = dismiss** (progressive fill
    animation during the hold; **visual-only** completion confirmation — no haptic in v1).
  - Guards: fire-time grace window (ignore proximity 1–2s after fire); pull-away hysteresis;
    sub-threshold reach = snooze, threshold = dismiss.
- **Input — rear buttons (3):** **Select** (cycle presets), **Arm** (toggle; long-press disarm
  all), **Adjust** (±5 min nudge). **Both outer buttons held ~3–5s = enter setup AP mode.**
- **Audio:** **passive piezo + RTTTL** as the primary v1 wake sound (one PWM/LEDC GPIO). Reserve
  PCB pads for an **I²S DAC + speaker** so v2 can add real audio without a respin.
- **Sensors:** VCNL4040 (proximity + lux) + DS3231 internal temp. (BME280 optional v2.)
- **Programming/debug:** native USB-C (USB-JTAG) + a few test points / UART header.
- **PCB:** 2-layer KiCad, 0805 passives, module's onboard antenna, hand-soldered v1. Fab TBD
  (JLCPCB cheap vs Aisler EU/fast).
- **Enclosure:** bedside landscape **wood-veneer bar**; dot-matrix glows through veneer; recessed
  rear buttons; concealed USB-C; hidden fasteners; **possible hidden IR window** if veneer blocks
  the proximity sensor's IR (see open questions).

### Configuration & HA integration
- **Config access:** **(a)** hold both rear buttons ~3–5s → **WiFi AP + captive portal** (first
  boot / WiFi change / recovery; self-contained, no network dependency; auto-exits on save or
  timeout). **(b)** everyday: **mDNS `.local`** web UI in a browser. **No NFC.**
- **Web UI** (self-hosted via `esp-idf-svc` HTTP server): WiFi creds, **define/edit presets**,
  proximity sensitivity, brightness curve, reveal duration, MQTT/HA details. Persisted in **NVS**.
- **HA transport:** **MQTT + auto-discovery + LWT availability.** Chosen over REST/webhook and
  over broker-free native-API/HomeKit (those require reimplementing ESPHome's protobuf API or
  HomeKit in Rust — a v3-scale detour). Broker creds entered once via the captive portal.
- **Full parity:** everything doable locally (gesture, buttons, web UI) is also doable from HA —
  set/edit/enable presets, arm/disarm, snooze, dismiss, read all state. Device auto-appears in HA
  once pointed at the broker.
- **Source-of-truth rule:** device owns state; local actions and HA commands are both just inputs
  into the same on-device state machine; publish state on every change (retained); HA never holds
  authoritative state; offline → local control unaffected, resync on reconnect.

### Presets
- Several premade alarms (e.g. Work / Weekend / Nap), defined in the web UI, stored in **NVS**,
  selected via the rear Select button, **fired on-device**. One preset data model mutated by all
  three front-ends (buttons, web UI, MQTT) so they never diverge.

---

## Firmware structure (target)

**Crates:** `esp-idf-hal`, `esp-idf-svc` (WiFi STA+AP, HTTP server, mDNS, MQTT, SNTP, NVS),
`esp-idf-sys`, `embedded-hal`, a dot-matrix/`smart-leds` driver (APA102/SK9822) + `embedded-graphics`,
`ds323x` (DS3231), a VCNL4040 driver.

**FreeRTOS tasks (Rust threads):**
- **Network thread** — MQTT client (discovery, state publish, command subscribe, LWT), HTTP/web UI,
  mDNS, AP/captive-portal when in setup mode, SNTP. Reconnects without blocking anything else.
- **Alarm/time thread** — *source of truth*. Reads DS3231, evaluates the armed preset, fires the
  alarm, runs the snooze/dismiss state machine. Independent of the network (offline reliability).
- **Interaction thread** — reads VCNL4040 (gesture classification + lux), reads rear buttons,
  drives reveal/snooze/dismiss + brightness.
- **Display thread** — renders time / alarm time / preset name / armed / dismiss-progress / "SETUP"
  at fixed refresh; handles fades.

Shared state via `Arc<Mutex<…>>` or channels. **The alarm thread never blocks on the network
thread.** Presets + settings in NVS.

**Toolchain:** `espup` (Xtensa S3 fork) → `cargo`; scaffold with `esp-idf-template`
(cargo-generate); `espflash`/`cargo-espflash` to flash; `probe-rs` or IDF monitor over native USB.
Reference: Espressif "Embedded Rust on ESP" book (covers WiFi + MQTT).

---

## Open questions (to resolve)

1. **Display panel sourcing** — pre-made diffused matrix module (easier, recommended v1) vs custom
   LED grid on the PCB (more analog freedom, v2-scale soldering). Plus grid size / field layout.
2. **Veneer IR passthrough** — BENCH TEST: does the VCNL4040 read reliably through the chosen wood
   veneer? If not → hidden IR window, or fall back to felt/frosted-acrylic front. (Darkness is NOT
   the issue — LEDs are simply off; IR transmission is.)
3. **Preset model specifics** — how many presets; per-preset fields (repeat days, sound, sunrise
   on/off). Drives the HA entity set + NVS schema. (Best next paper decision — feeds firmware.)
4. **Rough dimensions** of the bedside bar (drives panel size + enclosure).
5. **Budget + fab** — JLCPCB (cheap) vs Aisler (EU/fast); cost ceiling for BOM.

---

## Build sequence

1. **Bench validation first (~€60).** Dev-kit ESP32-S3 + VCNL4040 + DS3231(supercap) +
   MAX7219/APA102 warm matrix + piezo + 3 buttons + **wood veneer samples** + frosted-acrylic/felt
   fallbacks + breadboard. Goal: prove **IR-through-veneer**, the proximity gesture, and the warm
   glow before committing to a PCB. (Piezo only — defer I²S audio.)
2. Write/validate firmware on the breadboard: gesture classification + guards, presets in NVS,
   web UI + captive portal + mDNS, on-device firing, MQTT discovery + parity + LWT, RTC boot
   validity + NTP resync, offline behaviour.
3. Capture the proven circuit as a KiCad schematic; ERC.
4. Lay out the 2-layer PCB; DRC; export Gerbers; order.
5. Bring up the bare board incrementally: power rail → MCU enumerates → peripherals one by one.
6. Iterate the wood-veneer enclosure around the populated board.

---

## v2 / stretch (leave design hooks on v1)

- I²S DAC + speaker pads (real audio / web radio / TTS)
- Haptic motor (LRA + DRV2605L) for silent dismiss confirmation
- Light-ramp / sunrise wake before the piezo
- RGB ambient status backlight
- BME280 climate logging
- Active NFC reader (PN532) for token-based preset selection
- ESPHome-native-API or HomeKit in Rust for broker-free discovery

---

## Working preferences (from this project's collaborator)

- Wants to do the design/learning themselves; **minimal heavy AI usage** — use AI for sanity-checks,
  error explanations, and "review what I built," not for writing the core firmware/schematic/layout.
  Struggle through the thing being learned first; reach for help when genuinely stuck.
- Decision-oriented, prefers structured options with clear trade-offs.
- Based in the Netherlands (EU sourcing: Tinytronics / Antratek / AliExpress).

## Immediate next step for a code session

Scaffold the Rust project with `esp-idf-template` for the ESP32-S3, set up the `espup` toolchain,
and stub the four-thread structure above — but keep the collaborator driving (they're learning Rust
embedded + want to write the core themselves). The bench validation (step 1) gates real hardware
work, so early firmware can be developed against a dev-kit board in parallel.
