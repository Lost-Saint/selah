# Selah support matrix

The maintainer verifies on an **iD14 MKII** (`2708:0008`) only. Every other
model is cataloged from MixiD IDs and needs a community hardware result
before Selah claims it works.

| Model | USB | Selah status | Notes |
| --- | --- | --- | --- |
| iD14 MKII | `2708:0008` | Verified primary | Monitor, mixer, routing, readback + meters per `docs/protocol.md` |
| iD24 | `2708:000d` | Reference-derived | Evidenced analog routing + ADAT/S/PDIF mode; needs community listening test |
| iD4, iD4 MKII, iD14, iD22, iD44, iD44 MKII, iD48 | various | Cataloged — unverified | IDs + counts known; unavailable controls stay hidden, nothing claimed |

## How to contribute a result

1. Run Selah from this repo; copy the diagnostics block from the device panel.
2. Note model, `bcdDevice` from `lsusb -v -d 2708:<pid>`, what you tried, what you heard, and whether PipeWire/ALSA kept working.
3. Open an issue with that text. Do not paste serial numbers, full `lsusb -v` output, or USB captures.
