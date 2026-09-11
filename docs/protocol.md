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

Control requests have not been ported yet. Add verified request details here as the Rust transport is implemented; do not infer support from the C++ reference compiling or from a device being present in the catalog.
