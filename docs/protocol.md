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

Control-request execution is not enabled yet. Add verified request details here as the Rust transport is implemented; do not infer support from the C++ reference compiling or from a device being present in the catalog.

### Reference-derived speaker volume

MixiD sends speaker volume as a class/interface `SET_CUR` request with `wValue = 0x1200`, an output entity of `0x36`, and a two-byte little-endian signed value mapping normalized `0.0..=1.0` to `-32768..=-1`. Selah encodes this request as pure data and validates the input range. The opt-in hardware check `sends_harmless_speaker_volume_request` opens a safe session, sends level `0.1`, and closes the session:

```sh
SELAH_HARDWARE_PRODUCT_ID=0008 cargo test --test hardware_session -- --ignored --exact sends_harmless_speaker_volume_request
```

Treat this mapping as reference-derived until that check succeeds on the target model. It lowers the speaker level; run it only when that is safe for the connected setup.

## Hardware verification

| Date | Model | USB identity | Verification | Result |
| --- | --- | --- | --- | --- |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Claim and release application interface 4 without sending a control payload | Passed; audio interfaces 0–2 remained bound to `snd-usb-audio`, and PipeWire and ALSA still exposed playback and capture afterward |
| TBD | iD14 MKII | `2708:0008` | Send reference-derived speaker volume `0.1` via `sends_harmless_speaker_volume_request` | TBD — maintainer to fill after hardware run |

The device release comes from the USB descriptor and is not confirmed to be the user-facing firmware version.
