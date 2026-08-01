# Hardware

PCB and enclosure for the smart alarm clock, plus the breadboard **bench validation** that gates
the PCB. Bench board so far: **YD-ESP32-S3** devkit (ESP32-S3-WROOM-1). The custom PCB + wood-veneer
enclosure come after bench validation passes. Design rationale: [`../docs/handoff.md`](../docs/handoff.md).

## Components (v1)

- **MCU:** ESP32-S3-WROOM-1 (native USB-JTAG). Bench: YD-ESP32-S3 devkit.
- **Time:** SNTP + DS3231 RTC + supercap backup (no battery).
- **Power:** USB-C mains only, 5V → 3.3V buck (supercap backs the RTC only).
- **Display:** monochrome dot-matrix, **time only**, behind warm wood veneer, OFF when idle. Bench: red MAX7219 32×8; final: a warm/amber emitter (PCB-stage). Status (armed/ringing/…) via a single RGB LED (onboard WS2812).
- **Interaction:** native ESP32-S3 **capacitive touch** (foil/copper electrode behind the veneer, tap/hold) — senses through wood, so no IR-passthrough problem. Rear buttons available.
- **Audio:** passive buzzer via LEDC/PWM (BS170 low-side switch). Real audio is a future option (below).

## Bench validation

Get the device working end-to-end on a breadboard before committing to a PCB. Every part below has a
firmware hook that already exists (scaffolded) or is the immediate next thing to wire. Uses the
**existing dev board** and **on-hand buttons**. EU sourcing (Tinytronics / Antratek / AliExpress);
prices are rough guides. Wiring diagram + pin plan: **[bench-wiring.md](bench-wiring.md)**.

### Parts

Single-sourced at **Tinytronics** except the veneer (and, later, copper tape).

| Part | Serves | Firmware hook | ~€ |
|---|---|---|---|
| **LED Matrix 32×8 with MAX7219** (red, 3-wire SPI-like; SKU 003516) | shows `HH:MM` (time only) — validate 4-digit layout/legibility/geometry behind veneer | matrix time render (to wire) | 6 |
| **Keyestudio DS3231 RTC** module (I²C, SKU 005849) | correct time on boot + offline alarm firing | RTC read/set fallback (scaffolded) | 4 |
| **Passive Buzzer 3-12V — Dupont-Jumper** (passive → PWM tones) | v1 wake sound | LEDC/PWM (`buzzer.rs`) | 1 |
| **4-channel Bi-Directional Logic Level Converter** (3.3↔5 V, BSS138) | reliable data into the 5 V MAX7219 from 3.3 V ESP32 | — | 1.5 |
| **Capacitive-touch electrode** — kitchen foil / jumper (proper copper tape later, *not* at Tinytronics) | tap + hold gestures via the ESP32-S3's **native** touch peripheral, no sensor chip | touch → `Command` bus (to wire) | ~0 |
| **Wood veneer sample pack** (~0.6 mm; walnut/oak/etc.) — *source elsewhere* | display glow-through look **and** capacitive-touch-through-veneer tuning | — | 5–10 |
| Decoupling caps (100 nF / 10 µF) | supporting | — | 2 |
| *Optional:* single **SK6812 / WS2812** discrete pixel | final placement of the RGB status LED (onboard WS2812 covers the bench) | already driven in `led.rs` | 1–2 |
| *Optional:* **Logic Analyzer 8-channel USB** | SPI/I²C bring-up debugging (sigrok/PulseView) | — | 9.75 |

**Core subtotal: ~€12.50** (matrix + RTC + buzzer + level converter), + ~€10 optional logic analyzer.
**Spares:** order **2× MAX7219 matrix** and **2× level converter** (1 spare each, +€7.50) — these
parts don't wear out, but a wiring slip in the mixed 3.3/5 V path plus a reorder's shipping/wait
dwarfs the part cost.

**Status (2026-07-27):** parts ordered + arrived (electronics); wood veneer still to source. Buzzer
firmware written; breadboard bring-up is waiting on a soldering iron to attach the dev-board header.

### Wiring notes

- **DS3231 at 3.3 V, not 5 V** — its onboard I²C pull-ups reference VCC; 5 V would over-volt the S3's pins. It keeps perfect time at 3.3 V. I²C scan should ACK at `0x68`.
- **MAX7219 is 5 V logic** — drive DIN/CLK/CS through the level shifter; power the matrix from `5Vin`, keep brightness modest on USB.
- **Buzzer** — `GPIO4` → BS170 gate (low-side switch), drain → buzzer → `5Vin`. Gate held low by the ESP32-S3's internal pulldown; no external resistor.
- **Capacitive touch** — foil electrode on `GPIO14`; **no pull resistor** (it interferes with sensing). Fallback if touch feels wrong: a GY-APDS-9900 IR-reflective proximity module (I²C, ~€3.50), which reintroduces the IR-through-veneer test.

### Bring-up sequence

1. Solder the dev-board header, then bring up peripherals **one at a time**: RTC (I²C scan → `0x68`) → buzzer → MAX7219 `HH:MM` render → capacitive touch. Prove the `HH:MM` glow, capacitive-touch-through-veneer, and the warm look before a PCB.
2. Flesh out firmware on the breadboard: RTC read, matrix render, touch input + tuning, NVS persistence.
3. KiCad schematic → ERC → 2-layer layout → DRC → Gerbers → order.
4. Bring up the bare board incrementally; iterate the veneer enclosure.

### Deferred to PCB stage (bench uses the simpler thing)

- **RTC backup: supercap instead of the coin cell.** Wire the supercap on the DS3231 **VBAT** pin (across the coin-cell pads), *not* VCC — VBAT is the low-power (~1 µA) backup input. The DS3231 does **not** charge VBAT, so add a trickle path from VCC: `VCC → R (few hundΩ–1k) → Schottky → supercap → GND`, cap rated ≥ VCC (VBAT max 5.5 V). For the bench, just use the CR1220.
- **Warm display emitter.** The bench matrix is red; source a warm/amber ~32×8 emitter once the veneer test confirms the geometry.

## Future option — real audio / HA media player

The v1 buzzer could later be replaced by real audio: a **default tone stored on device** (offline
wake sound), **user-uploadable sound files**, and registering as an **HA `media_player`** (HA pushes
TTS / chimes / radio). Parts to add (~€20): an **ESP32-S3-DevKitC-1 N16R8** (16 MB flash for uploads +
8 MB PSRAM to buffer/decode streams — the current board has neither), a **MAX98357A** I²S amp, and a
3–4 W speaker. Open decisions: WAV vs MP3 for uploads; Rust vs ESPHome for the `media_player` (the
Rust path is a large firmware lift, ESPHome provides it natively but would replace the current firmware).

## PCB & enclosure (after bench validation)

- 2-layer KiCad schematic → ERC → layout → DRC → Gerbers → fab (JLCPCB vs Aisler EU). 0805 passives, module antenna, hand-soldered v1.
- Wood-veneer bar enclosure: matrix glows through the veneer; concealed USB-C; hidden fasteners.

## Open questions

- Warm display-emitter sourcing (bench uses red).
- Capacitive-touch-through-veneer tuning (electrode size, thresholds, groundless USB supply).
- Per-slot fields (repeat days / sound / sunrise) → drives the HA entity set + NVS schema.
- Enclosure dimensions; budget + fab (JLCPCB vs Aisler).
