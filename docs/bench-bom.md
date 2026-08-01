# Bench Bring-Up BOM

The concrete parts list for **Build sequence step 1** in [`handoff.md`](handoff.md): get the
device working end-to-end on a breadboard before committing to a PCB. Every item here has a
firmware hook that already exists (scaffolded) or is the immediate next thing to wire.

Sourcing is EU (Tinytronics / Antratek / AliExpress). Prices are rough guides, not quotes.

Uses the **existing ESP32-S3 dev board** (currently running the base firmware) and the
**buttons already on hand** — neither is re-ordered.

---

## Order now

| Part | Serves | Firmware hook | ~€ |
|---|---|---|---|
Single-sourced at **Tinytronics** except the veneer (and, later, copper tape).

| Part | Serves | Firmware hook | ~€ |
|---|---|---|---|
| **LED Matrix 32×8 with MAX7219** (red, 3-wire SPI-like; Tinytronics SKU 003516) | shows `HH:MM` (time only) — validate 4-digit layout/legibility/geometry behind veneer | LED render (matrix time layout, to wire) | 6 |
| **Keyestudio DS3231 RTC** module (I²C, Tinytronics SKU 005849) | correct time on boot + offline alarm firing | RTC read/set fallback (scaffolded) | 4 |
| **Passive Buzzer 3-12V — Dupont-Jumper** (passive → PWM tones; Tinytronics) | v1 wake sound | LEDC/PWM audio (to wire) | 1 |
| **4-channel Bi-Directional Logic Level Converter** (3.3↔5 V, BSS138) for the MAX7219 DIN/CLK/CS | reliable data into the 5 V MAX7219 from 3.3 V ESP32 | — | 1.5 |
| **Capacitive-touch electrode** — kitchen foil / jumper for the bench (proper copper tape later, *not* at Tinytronics) | reach/tap + hold gestures via the ESP32-S3's **native** touch peripheral, no sensor chip | touch → `Command` bus (to wire) | ~0 |
| **Wood veneer sample pack** (~0.6 mm; walnut/oak/etc.) — *source elsewhere* | display glow-through look **and** capacitive-touch-through-veneer tuning | — | 5–10 |
| Decoupling caps (100 nF / 10 µF) | supporting | — | 2 |
| *Optional:* single **SK6812 / WS2812** discrete pixel | final placement of the RGB status LED (onboard WS2812 covers the bench) | already driven in `software/firmware/src/led.rs` | 1–2 |
| *Optional:* **Logic Analyzer 8-channel USB** (Tinytronics) | SPI/I²C bring-up debugging (sigrok/PulseView) | — | 9.75 |

**Tinytronics core subtotal: ~€12.50** (matrix + RTC + buzzer + level converter). + ~€10 for the
optional logic analyzer. Veneer sourced elsewhere. Uses the existing ESP32-S3 dev board and
on-hand buttons.

**Spares:** order **2× MAX7219 matrix** and **2× level converter** (1 spare each, +€7.50). These
parts don't wear out — the realistic failure is a wiring slip in the mixed 3.3/5 V path — and a
reorder's shipping + wait dwarfs the part cost, so the spare is cheap insurance against stalling.

**Status: ordered (2026-07-25)** — core kit + spares placed at Tinytronics. Still to source
elsewhere: wood veneer samples (now), copper tape (later; foil/jumper works for the first
cap-touch test).

### Display split (decided)
- **Matrix = time only.** A single 8×8 can't render 4 legible digits, so the panel is a **wide
  ~32×8**. The bench module is a **red MAX7219 4-in-1** — cheap and well-supported. It fully
  validates layout / legibility / geometry behind the veneer; the *warm* emitter (amber) is a
  separate **PCB-stage** decision, since red vs amber diffuse differently through wood so the red
  panel is only an *approximate* glow test.
- **Status (armed / AP mode / ringing / etc.) = a single RGB LED**, not the matrix. This is
  **already implemented** — `software/firmware/src/led.rs` drives the onboard WS2812 with phase colors
  (syncing=blue, idle=rainbow, armed=green, ringing=red, snoozed=amber). Onboard pixel covers the
  bench; add one discrete SK6812/WS2812 for final placement.

### Gesture input (decided) — native capacitive touch, no sensor chip
- The **ESP32-S3 touch peripheral** (14 channels on GPIO1–14, incl. a proximity-sensing mode)
  drives the input from a **copper electrode behind the veneer**. Capacitive sensing works
  *through* wood (a thin dielectric), which **removes the IR-through-veneer PCB blocker entirely**.
- **Interaction shifts to touch/near-hover** (touch the wood surface) rather than a non-contact
  hand-wave: quick tap = reveal/snooze, ~2.5 s hold = dismiss.
- **Bench task:** size the electrode + tune firmware threshold/hysteresis/filtering through the
  veneer. Capacitive drifts with temp/humidity and wants a decent ground reference (USB-mains, no
  earth — workable with filtering).
- **Fallback if touch feels wrong:** the **GY-APDS-9900** IR reflective proximity module (I²C,
  ~€3.50, Tinytronics) — same working principle as the originally-specced VCNL4040, but it brings
  back the IR-through-veneer test.

### Wiring notes
- **I²C bus:** just the DS3231 (0x68) for now. **Power the DS3231 module from 3.3 V, not 5 V** —
  its onboard I²C pull-ups reference VCC, and 5 V pull-ups would over-volt the ESP32-S3's 3.3 V
  pins. The DS3231 keeps perfect time at 3.3 V.
- **MAX7219 is 5 V logic** — its logic-high threshold at 5 V VCC (~3.5 V) is above the ESP32-S3's
  3.3 V output, so drive DIN/CLK/CS through the level shifter (or a 74HCT125). Power the matrix
  from the dev board's 5 V; keep brightness modest on USB.

---

## Deferred to PCB stage (bench uses the simpler thing)

- **RTC backup: supercap instead of the coin cell.** Fits the "no battery / seal-and-forget"
  intent. Wire the supercap on the DS3231 **VBAT** pin (across the coin-cell holder pads), *not*
  VCC — VBAT is the low-power (~1 µA) backup input the chip auto-switches to. The DS3231 does **not**
  charge VBAT itself, so add a trickle-charge path from VCC: `VCC → R (few hundΩ–1k) → Schottky →
  supercap → GND`, cap rated ≥ VCC (VBAT max 5.5 V). For the bench, just use the CR1220.
- **Warm display emitter.** The bench matrix is red; the final panel should be warm/amber to match
  the aesthetic. Source warm 32×8 tiles (or pick a warm emitter) once the veneer test confirms the
  geometry.

## Future option — real audio / HA media player

Deferred; **piezo covers v1**. Captured here so the intent isn't lost. Expands the existing
"I²S DAC + speaker pads" v2 hook into a fuller vision:

- **Default tone stored on device** — the offline wake sound; fires with no network (preserves the
  offline-first alarm guarantee).
- **Upload your own sound files** — user-provided audio stored on the device.
- **Home Assistant media API** — the device registers as an HA `media_player` so HA can push TTS
  announcements, chimes, radio, and music to it.

**Parts to add when this phase starts (~€20):**
- **ESP32-S3-DevKitC-1 N16R8** (16 MB flash + 8 MB PSRAM) — becomes the new main board. The
  **16 MB flash** is what makes user file-uploads viable (the current 4 MB board has no room after
  firmware + web UI), and the **8 MB PSRAM** is needed to buffer/decode streamed media. The current
  build enables no PSRAM, confirming the current module is the no-PSRAM variant.
- **MAX98357A** I²S class-D amp (DAC built in — I²S in, speaker out; no separate DAC stage).
- **Speaker** 3–4 W 4 Ω, ~40 mm+ (spend a little more here for media-player quality; mono for v1,
  stereo would need a second amp).

**Decisions to make before building it (not before ordering):**
1. **Upload file format** — WAV (simple to play, large) vs MP3 (compact, needs a decoder).
2. **Rust vs ESPHome for the `media_player`** — an HA-integrated streaming media player is a large
   firmware lift in Rust (HTTP stream client + audio decode + I²S DMA + the media_player entity &
   command protocol). ESPHome provides `i2s_audio` + `media_player` → native HA entity essentially
   for free, but would replace the current Rust firmware. Decide before building the Rust path.
