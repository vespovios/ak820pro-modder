# Local AK820 compatibility build

Based on v0.7.0-beta. Tested on macOS with a wired SONiX AK820,
VID `0x0C45`, PID `0x800A` (2026-10-23).

## The actual difference

This keyboard exposes interface 3, usage page `0xFF13`, with 64-byte
Input/Output/Feature reports. Interface 2 (`0xFF68`) is its 4096-byte
**display data** channel. Sending the upstream app's `AA 10 ...` device
information request to either interface produces no valid response.

The compatibility path selects `0xFF13` when present; other hardware keeps
the upstream output-report implementation. Product ID alone does not choose
the protocol. Explicit interface/usage-page environment overrides still work.

## Feature transactions

hidapi sends 65 bytes: one report-ID byte `00`, then the complete 64-byte
payload. The payload's leading `04` is **data**, not the HID report ID.
Every write settles for 35 ms and is followed by a feature-report read.

Settings follow `START (18) → setting preamble → data → SAVE (02) → FINISH (F0)`.
SAVE uses flag byte 8 = 0; other command packets use flag 1. Command responses
must contain `04 <command> <sub> 01 ...`; status `FF`, echoed requests, short
reports and I/O errors fail the operation. FINISH is attempted after errors.
No configuration or SAVE payload is sent by connection verification.

The SAVE step is important: this physical unit returned status `FF` when
FINISH followed lighting data directly. Adding SAVE produced status `01`
for both SAVE and FINISH. The rejected sequence is covered by regression tests.

The static effect uses breathing with speed zero. Off uses single-on with
brightness zero. Lighting data ends with `55 AA` at offsets 14–15; clock
and sleep data use `AA 55` at offsets 62–63.

## Feature limits

The known setters support global lighting, display clock, and sleep. They
provide transaction acknowledgements but do not expose decoded getters for
firmware, battery or current settings. The app does not invent telemetry or
label last-submitted values as read-back. Onboard keymap and macro uploads and audio-reactive mode remain
unavailable for this protocol. [Mac-side controls](MAC-SIDE-CONTROLS.md)
provide key remapping and local macro recording/playback instead. Presets
allow lighting and app automation components, with onboard overrides disabled.
ANSI per-key RGB now uses a separately recovered colour-bank transaction;
see [Per-key RGB](PER-KEY-RGB.md) for its protocol and validation evidence.

## Verification

- The physical keyboard returned valid START and FINISH acknowledgement packets.
- The library lighting smoke test completed START, MODE, data, SAVE and FINISH.
- The packaged app verified interface 3 and successfully submitted clock sync,
  steady purple lighting and a 30-minute standby setting, with firmware
  acknowledgements and no timeout banners.
- All 63 workspace unit tests and the TypeScript/production frontend build passed.
- Automated tests check real packet framing, static/off special cases, valid
  dates, the four sleep codes, failed acknowledgements, commit ordering and
  cleanup after a rejected preamble.
- Run the default (settings-neutral) probe with:
  `cargo run -p ak820-protocol --example feature_probe --release`.
- Add `-- --test-lighting` only to deliberately apply steady purple.

Physical colour and clock appearance require checking the keyboard itself;
USB acknowledgements establish that its firmware accepted the transaction.

## TFT display

The feature-report path uses AKS075 image framing: START with flag 0,
IMAGE_CFG (`04 72 03`) with the little-endian block count at offsets 8–9,
raw 4096-byte reports on the matching FF68 endpoint, then SAVE. Each block
waits up to one second for a data input report. Missing acknowledgements
abort the upload without SAVE; FINISH is attempted to close a failed session.
Image transactions are not automatically retried after partial transfer.

The payload begins with 256 bytes: frame count, one delay byte per frame,
then FF padding. Static images use delay 0. Animated delays use 2 ms units,
including the final frame's delay. Row-major RGB565 little-endian pixels
follow, with FF padding to complete the final block. This differs from the
upstream AA protocol's chunk header and timing convention.

The local UI decodes PNG/JPEG/WebP and GIF, centre-crops to 128×128,
composites transparency onto black, and previews the encoded RGB565 pixels.
Limits are 10 MB source files, 4096×4096 source dimensions and 30 frames.
GIF delays are normalized to 2–510 ms in 2 ms steps, with an explicit notice;
zero-delay frames use 100 ms. Longer animations are rejected rather than
silently truncated. Images are processed locally without network upload.

The physical 800A unit acknowledged all nine blocks and SAVE for a static
four-colour image. The user confirmed the image appeared on its screen.
The final packaged UI also loaded a two-frame GIF through the native file
picker, displayed its animated RGB565 preview, and received all 17 data
acknowledgements plus SAVE during upload.
The user confirmed the keyboard screen alternated between the GIF's purple
and cyan frames.

## References

Protocol facts were cross-checked against these MIT-licensed implementations:

- [macOS feature-report implementation](https://github.com/gusleig/ajazz-ak820-pro-mac/blob/main/ak820/control.py)
- [WebHID implementation supporting 8009 and 800A](https://github.com/kabelklaus/ajazz-ak820-pro/blob/main/app.js)

This build does not flash or replace firmware.
