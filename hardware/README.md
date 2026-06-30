# Hardware

KiCad schematic, PCB layout, and fabrication outputs for the smart alarm clock.
Empty for now — populated after bench validation proves the circuit (see the
build sequence in the root `README.md` and `docs/handoff.md`).

Planned contents:

- `*.kicad_pro` / `*.kicad_sch` / `*.kicad_pcb` — 2-layer board (ESP32-S3-WROOM-1)
- BOM + fab notes (JLCPCB vs Aisler — open question #5)
- Gerbers / production exports

Key parts (v1): ESP32-S3-WROOM-1, DS3231 RTC + supercap, VCNL4040, warm
APA102/SK9822 dot-matrix, passive piezo, 5V→3.3V buck, USB-C, 3 rear buttons.
Reserve pads for a v2 I²S DAC + speaker.
