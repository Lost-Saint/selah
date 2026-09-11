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

Selah currently enumerates USB descriptors and filters them by Audient's vendor ID. Discovery does not open the device, claim a USB interface, or detach a kernel driver. Known product IDs are matched to the static catalog; unknown Audient product IDs remain visible to the UI as unsupported devices.

For known devices, discovery also looks for an application-specific (`0xfe`) or vendor-specific (`0xff`) USB interface. A device session may claim one of those interfaces without detaching a kernel driver. Selah intentionally refuses to fall back to an audio-class interface until that behavior can be designed and verified safely.

Control requests have not been ported yet. Add verified request details here as the Rust transport is implemented; do not infer support from the C++ reference compiling or from a device being present in the catalog.
