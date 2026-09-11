# Audient protocol notes

Selah's initial device catalog is based on the open-source [MixiD device properties](https://github.com/TheOnlyJoey/MixiD/blob/master/device_properties.h). MixiD remains the primary reference until behavior is independently verified against physical hardware.

## USB identity

- Vendor ID: `0x2708`
- Product IDs and channel capabilities live in `src/device/catalog.rs`.

## Implementation rules

- Keep request encoding and decoding pure and unit-tested.
- Keep USB handle ownership, interface claiming, and kernel-driver restoration at the transport boundary.
- Prefer a spare DFU or vendor interface so the kernel audio driver can keep streaming. Any fallback that detaches a kernel driver must restore it on every exit path.
- Record hardware verification with the exact interface model and relevant firmware version.

## Discovery

Selah currently enumerates USB descriptors and filters them by Audient's vendor ID. An operating-system hot-plug monitor triggers a fresh descriptor scan when any USB device connects or disconnects; it does not poll. Discovery does not open the device, claim a USB interface, or detach a kernel driver. Known product IDs are matched to the static catalog; unknown Audient product IDs remain visible to the UI as unsupported devices.

For known devices, discovery also looks for an application-specific (`0xfe`) or vendor-specific (`0xff`) USB interface. A device session may claim one of those interfaces without detaching a kernel driver. Selah intentionally refuses to fall back to an audio-class interface until that behavior can be designed and verified safely.

Control-request execution is limited to speaker and headphone volume: the UI sliders and the opt-in hardware checks send the reference-derived requests below through serialized background tasks. Do not infer support for other controls from the C++ reference compiling or from a device being present in the catalog.

### Reference-derived speaker volume

MixiD sends speaker volume as a class/interface `SET_CUR` request with `wValue = 0x1200`, an output entity of `0x36`, and a two-byte little-endian signed value mapping normalized `0.0..=1.0` to `-32768..=-1`. Selah encodes this request as pure data and validates the input range. The UI slider sends it through a background task that opens a safe session, sends one bounded request, and closes the session; the slider position is the last requested level, and the status line reports only what was sent, since Selah cannot read the level back. The opt-in hardware check `sends_harmless_speaker_volume_request` exercises the same path with level `0.1`:

```sh
SELAH_HARDWARE_PRODUCT_ID=0008 cargo test --test hardware_session -- --ignored --exact sends_harmless_speaker_volume_request
```

Treat this mapping as reference-derived until the audible change is confirmed on speakers. It lowers the speaker level; run it only when that is safe for the connected setup. Transfer acceptance was verified on an iD14 MKII on 2026-09-11 (see table); the audible level change itself is unconfirmed because that setup has headphones only and no speakers.

### Reference-derived headphone volume

MixiD `set_hp_volume` (driver.h) sends the headphone level twice — controls `0x0203` and `0x0204` against output entity `0x0a` — with the same two-byte little-endian signed mapping (`0.0..=1.0` to `-32768..=-1`). Selah encodes both transfers as pure data, validates the input range, and sends them in order on one session so a first-transfer failure is reported instead of leaving the channels mismatched. The UI slider sends them through a background task that opens a safe session, sends both bounded requests, and closes the session; the slider position is the last requested level, and the status line reports only what was sent, since Selah cannot read the level back. The opt-in hardware check `sends_harmless_headphone_volume_request` exercises the same path with level `0.1`:

```sh
SELAH_HARDWARE_PRODUCT_ID=0008 cargo test --test hardware_session -- --ignored --exact sends_harmless_headphone_volume_request
```

Treat this mapping as reference-derived until the audible change is confirmed on headphones. It sets the headphone level; run it only when that is safe for the connected setup.

## Hardware verification

| Date | Model | USB identity | Verification | Result |
| --- | --- | --- | --- | --- |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Claim and release application interface 4 without sending a control payload | Passed; audio interfaces 0–2 remained bound to `snd-usb-audio`, and PipeWire and ALSA still exposed playback and capture afterward |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Send speaker volume `0.1` via `sends_harmless_speaker_volume_request` | Passed transfer; audio interfaces 0–2 remained bound to `snd-usb-audio`, and PipeWire still exposed playback and capture afterward. Audible change unconfirmed — headphones-only setup, no speakers connected |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | 5 back-to-back passes of claim/release plus speaker-volume `0.1`, then a busy-interface probe (raw holder claim, Selah open must fail `Busy` with recovery hint, then open/close recovery) | Passed; 10/10 session cycles and the Busy kind, hint, and recovery open/close all succeeded. Audio interfaces 0–2 stayed bound to `snd-usb-audio` and PipeWire kept exposing playback and capture throughout |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Maintainer-reported: quit the Selah app while connected, then physical unplug/replug with the app running | Passed; PipeWire playback and capture kept working, and Selah picked the device back up after the replug. Agent-corroborated after the fact: device re-enumerated, interfaces 0–2 bound to `snd-usb-audio`, PipeWire still exposing the iD14 |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Send headphone volume `0.1` via `sends_harmless_headphone_volume_request`, once plus 5 back-to-back repeats | Passed transfer 6/6; audio interfaces 0–2 remained bound to `snd-usb-audio`, and the PipeWire Headphones sink stayed `RUNNING` with the same active stream throughout. Audible level change unconfirmed — the agent has no way to hear the output; needs maintainer corroboration |

The device release comes from the USB descriptor and is not confirmed to be the user-facing firmware version.
