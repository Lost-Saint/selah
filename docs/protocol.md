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

Implemented controls use pure reference-derived encoders and serialized requests on short-lived background sessions. Monitor state and meters additionally read back through bounded polls on the same session path; mixer, routing, polarity, and headphone state stay write-only because their reads are untrustworthy. Routing is enabled only by an explicit per-model capability map; a product ID or output count alone never enables a write.

### Reference-derived speaker volume

MixiD sends speaker volume as a class/interface `SET_CUR` request with `wValue = 0x1200`, an output entity of `0x36`, and a two-byte little-endian signed value mapping normalized `0.0..=1.0` to `-32768..=-1`. Selah encodes this request as pure data and validates the input range. The UI slider sends it through a background task that opens a safe session, sends one bounded request, and closes the session. The same node answers `GET_CUR` with two bytes in the same encoding (BiD `get_monitor_volume`), so the slider adopts the hardware value as confirmed state: front-panel moves and reconnects appear instead of being overwritten. The opt-in hardware check `sends_harmless_speaker_volume_request` exercises the same path with level `0.1`, and `reads_monitor_volume_harmlessly` probes the read without writing anything:

```sh
SELAH_HARDWARE_PRODUCT_ID=0008 cargo test --test hardware_session -- --ignored --exact sends_harmless_speaker_volume_request
```

Treat this mapping as reference-derived until the audible change is confirmed on speakers. It lowers the speaker level; run it only when that is safe for the connected setup. Transfer acceptance was verified on an iD14 MKII on 2026-09-11 (see table); the audible level change itself is unconfirmed because that setup has headphones only and no speakers. The `GET_CUR` readback of this node passed on the same model on 2026-09-12, returning the exact residue of a `0.1` write.

### Reference-derived headphone volume

Headphone volume lives on feature unit `0x0c`: selector `0x02`, channels 3 and 4 (channels 1 and 2 are the monitor pair). Selah sends the level to controls `0x0203` and `0x0204` with the same two-byte little-endian signed mapping (`0.0..=1.0` to `-32768..=-1`), in order on one session so a first-transfer failure is reported instead of leaving the channels mismatched.

Cautionary history: MixiD `set_hp_volume` (driver.h) used the same controls but against output entity `0x0a`, and Selah initially reproduced that mapping. The [BiD fork](https://github.com/baakhoff/BiD) found that entity `0x0a` declares no controls at all — those writes are accepted by USB and do nothing audible — and corrected the entity to `0x0c`, matching the unit and channel counts read from real iD14 MKII descriptors (see its `docs/PROTOCOL.md`, "What an iD14 MKII's descriptors say"). Transfer acceptance alone therefore proves nothing for this control; only a listening test counts.

The UI slider sends both bounded requests through a background task that opens a safe session, sends, and closes the session; the slider position is the last requested level, and the status line reports only what was sent. Headphone level stays write-only deliberately: the [BiD reference](https://github.com/baakhoff/BiD/blob/5a60eced59115bad4745aa7416056891252e4173/docs/PROTOCOL.md) documents that `GET_CUR` on the mixer, routing, channel-phase, and headphone entities stalls or returns aliased junk, so Selah never reads those entities and never presents their last-sent values as confirmed. The opt-in hardware check `sends_harmless_headphone_volume_request` exercises the same path with level `0.1`:

```sh
SELAH_HARDWARE_PRODUCT_ID=0008 cargo test --test hardware_session -- --ignored --exact sends_harmless_headphone_volume_request
```

Treat this mapping as reference-derived until the audible change is confirmed on headphones. It sets the headphone level; run it only when that is safe for the connected setup. One known limitation from BiD's measurements: on a cue-fed phones output the `0x0c` headphone gain made no audible difference, and the Main-Mix-fed case is untested there — so audibility may also depend on what the phones output is currently routed to, which Selah cannot see yet.

### Reference-derived output routing

Routing assigns a source to each physical output; Main Mix, Alt Speaker, Cue A, Cue B, and DAW Mix are sources, not grid coordinates. Entity `0x33`, selector `0x06`, takes one byte per output channel. Selah sends the two halves of a stereo destination in order on one session and stops after the first failed transfer. The current route cannot be read back reliably, so the UI distinguishes the last requested source from a transfer the device accepted and never calls either confirmed hardware state.

There is no known route-off wire value. [MixiD issue #11](https://github.com/TheOnlyJoey/MixiD/issues/11) records that an output is always in one source state. Selah therefore offers **Reset** as the undo action: it sends the documented default for that physical output pair instead of inventing an off code.

The iD14-family table comes from MixiD's `routeToggle`. Selah enables it only for the iD14 MKII, whose six-output routing unit and table were subsequently checked on physical hardware by the [BiD reference](https://github.com/baakhoff/BiD/blob/5a60eced59115bad4745aa7416056891252e4173/docs/PROTOCOL.md):

| Output channels | Selah destination | Main | Cue A | Cue B | DAW Mix | Reset |
| --- | --- | --- | --- | --- | --- | --- |
| `0,1` | Main speakers / outputs 1–2 | `0x1b,0x1c` | `0x19,0x19` | `0x1a,0x1a` | `0x00,0x01` | Main Mix |
| `2,3` | Line outputs 3–4 | `0x1b,0x1c` | `0x19,0x19` | `0x1a,0x1a` | `0x02,0x03` | DAW Mix |
| `4,5` | Headphones | `0x1b,0x1c` | `0x19,0x19` | `0x1a,0x1a` | `0x04,0x05` | Cue A |

The iD14 MKII has no alternate-speaker output, so Selah does not expose the otherwise present MixiD Alt codes. First-generation iD14 writes remain unavailable because the table has not been verified on that named model.

The iD24 mapping comes from BiD's documented listening tests and its official-app decode. Its named source bytes are Main `0x25/0x26`, Alt Speaker `0x27/0x28`, Cue A `0x1e/0x1f`, Cue B `0x20/0x21`, and DAW Mix equal to the zero-based output channel. The evidenced analog destinations use routing-unit channels `0,1`, `2,3`, and `4,5`. Research places an optical pair at non-contiguous routing channels `8,9`, but its reset state is not established, so Selah does not expose that pair as a selectable route.

The iD24's separately evidenced optical-output format request is exposed: entity `0x14`, selector `0x01`, channel zero, with a four-byte little-endian value (`0` = ADAT, `1` = S/PDIF). The node answers `GET_CUR` with the same layout (BiD `get_optical_mode`), so Selah reads the mode back on refresh and shows confirmed hardware state; any other value byte is rejected, not guessed. Other models keep this control unavailable until their entity mapping is confirmed.

The iD22 report in [MixiD issue #25](https://github.com/TheOnlyJoey/MixiD/issues/25) also matters on the input side: ADAT 1–8 follow the two onboard inputs as mixer rows 2–9 (zero-based), rather than restarting at row zero. Selah uses that typed ADAT-to-mixer mapping rather than deriving it from output-route indexes. The iD22, iD44 family, and iD48 routing source formulas remain unavailable because existing research does not provide hardware-confirmed values safe enough for writes; their digital-output and insert counts do not imply routing support.

Insert and send/return controls are not implemented. [MixiD issue #5](https://github.com/TheOnlyJoey/MixiD/issues/5) confirms that a mapping still needs capture and verification, so Selah exposes no speculative control.

### Reference-derived monitor toggles

MixiD `set_bool_state` (driver.h) flips one monitor bool with `wValue = masterVals[mode]`, entity `0x36`, and a one-byte payload. Selah encodes the five panel toggles the same way: Dim `0x0500`, Alt-speaker `0x0c00`, Talkback `0x0700`, Mono `0x0000`, and Speaker Mute `0x0400`. Each send opens a safe session, transfers one bounded request, and closes the session. The same selectors answer `GET_CUR` with the one-byte value the device holds (BiD `get_bool_state`), so each button adopts confirmed hardware state on refresh instead of tracking a local dummy array the way `MixiD` does. The strip is gated to the iD14 MKII (`0x0008`); do not widen without per-model hardware evidence.

Treat these mappings as reference-derived until each toggle is confirmed audibly on hardware. They change the monitor path; run them only when that is safe for the connected setup.

### Reference-derived input mixer matrix

`MixiD` `set_channel_volume` (driver.h) writes one input's Main-send pair as two cells on entity `0x3c`: `wValue = 0x0100 + channel * 6` (Main L) and `+ 1` (Main R), with the shared two-byte level payload. Selah sends both cells in order on one session, stopping on a first-transfer failure so the pair cannot mismatch silently. Input polarity follows `MixiD` `set_phase_state`: a one-byte bool with `wValue = 0x0d01 + channel` on entity `0x0b`.

`channel` is the running input index across microphone then digital inputs. This deliberately differs from `MixiD`'s UI (main.cpp), whose digital loop reuses its own loop counter and aliases digital channels onto microphone mappings; Selah numbers every input by its position in the full input sequence. Strip counts come from the catalog (`mic_inputs + digital_inputs`).

Three findings from the [BiD fork](https://github.com/baakhoff/BiD) shape this design. The level scale is dB in a u16 (`0x0000` is 0 dB, `0x8000` is mute), not a fraction: low fader values are silence, so audibility tests need high levels — a `0.1` test level sits near −115 dB and proves nothing by ear. The Main-send pair is the input's stereo image (the L/R ratio is the pan), so writing one level to both, as `MixiD` does, sums the input to the centre. And the matrix, routing, and phase entities do not read back, so the faders show the last requested value, never confirmed device state.

Channel mute, solo, and stereo linking have no known USB mapping in `MixiD`, BiD, or [Monix](https://github.com/sKuhLight/monix): Selah sends nothing for them and shows no control that pretends otherwise. DAW-return rows exist on the matrix but have no strips yet; Selah leaves rows it does not own untouched rather than silencing the user's computer audio on connect.

Treat these mappings as reference-derived until each strip is confirmed audibly on hardware. They change the monitor path; run them only when that is safe for the connected setup.

### Device readback and metering

Only the monitor entity answers reads truthfully. Everything below comes from the [BiD reference](https://github.com/baakhoff/BiD/blob/5a60eced59115bad4745aa7416056891252e4173/docs/PROTOCOL.md) (`driver.h` getters, "Reads do not work", "Watching the monitor section move"). The monitor-volume read, the meter-block probe, and a full GUI feedback session passed transfer-level on an iD14 MKII on 2026-09-12 (see table); meter level accuracy against a known signal and front-panel cross-checks of the toggle reads remain open:

- Monitor volume: `GET_CUR` (`0xa1`, `0x01`), `wValue 0x1200`, entity `0x36`, two bytes little-endian in the volume encoding. Toggles: `GET_CUR` with the same `wValue` as the write, one byte, nonzero means on.
- Optical-output mode: `GET_CUR`, `wValue 0x0100`, entity `0x14`, four bytes; first byte `0`/`1`. Anything else is rejected.
- Meters: `GET_MEM` (`0xa1`, `0x03`), `wValue 0x0000`, entity `0x3c`, one 32-byte block for sixteen input nodes; the first byte of each two-byte node is the level. A short block is rejected, never zero-filled.
- Write-only: mixer-matrix cells (reads alias across input rows), output routing, channel polarity, and headphone volume. Selah never issues those reads, so those controls show last-sent or unknown — never confirmed.

Selah polls readback on short-lived sessions through the same claim-and-release path as writes: an immediate full refresh on (re)connect, then meter blocks at 10 Hz with the monitor snapshot folded in every ~1 s, on one session per tick with at most one poll in flight and 100 ms timeouts. There is no continuous repaint loop and no busy poll: the timer exists only while a supported device with readable state is present, ticks arriving mid-poll are dropped, and unchanged values produce no new log or state noise. A failed meter block blanks the meters rather than freezing them; a failed monitor read keeps the last confirmed values under a retry notice; reconnect resets everything to unknown before readback succeeds.

One caution carried over from BiD (issue #26): control traffic while the firmware re-clocks after a sample-rate change wedged the control plane on an iD14 MKII. Selah's traffic is an order of magnitude below BiD's meter-plus-monitor polling, but treat rate-change windows as suspect if reads start stalling, and never add faster polling to chase it.

## Hardware verification

| Date | Model | USB identity | Verification | Result |
| --- | --- | --- | --- | --- |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Claim and release application interface 4 without sending a control payload | Passed; audio interfaces 0–2 remained bound to `snd-usb-audio`, and PipeWire and ALSA still exposed playback and capture afterward |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Send speaker volume `0.1` via `sends_harmless_speaker_volume_request` | Passed transfer; audio interfaces 0–2 remained bound to `snd-usb-audio`, and PipeWire still exposed playback and capture afterward. Audible change unconfirmed — headphones-only setup, no speakers connected |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | 5 back-to-back passes of claim/release plus speaker-volume `0.1`, then a busy-interface probe (raw holder claim, Selah open must fail `Busy` with recovery hint, then open/close recovery) | Passed; 10/10 session cycles and the Busy kind, hint, and recovery open/close all succeeded. Audio interfaces 0–2 stayed bound to `snd-usb-audio` and PipeWire kept exposing playback and capture throughout |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Maintainer-reported: quit the Selah app while connected, then physical unplug/replug with the app running | Passed; PipeWire playback and capture kept working, and Selah picked the device back up after the replug. Agent-corroborated after the fact: device re-enumerated, interfaces 0–2 bound to `snd-usb-audio`, PipeWire still exposing the iD14 |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Send headphone volume `0.1` via `sends_harmless_headphone_volume_request`, once plus 5 back-to-back repeats | Passed transfer 6/6; audio interfaces 0–2 remained bound to `snd-usb-audio`, and the PipeWire Headphones sink stayed `RUNNING` with the same active stream throughout. Audible level change unconfirmed — the agent has no way to hear the output; needs maintainer corroboration |
| 2026-09-11 | iD14 MKII | `2708:0008`, device release `0x0112` | Listening tests against the corrected `0x0c` headphone entity: send `0.1` then `1.0` while audio played, then drag the speaker slider to `0%` while listening on headphones | Both `0x0c` sends transferred cleanly with audio intact, but neither produced any audible change, and the speaker slider left the headphones unchanged too. Conclusion: this setup's phones are routed to a fixed-level feed (cue or DAW-thru), not Main Mix — so neither the `0x0c` headphone gain nor the `0x36` monitor control can bite until the phones are routed to Main Mix (Milestone 4 scope). Matches BiD's finding that `0x0c` ch3/4 is inaudible on cue-fed phones |
| 2026-09-12 | iD14 MKII | `2708:0008`, device release `0x0112` | Read monitor volume via `reads_monitor_volume_harmlessly` (`GET_CUR` `0x1200`/`0x36`, no writes) | Passed; decoded `0.100009`, the exact residue of the earlier `0.1` test write — the first proof a Selah volume write actually landed in hardware and reads back |
| 2026-09-12 | iD14 MKII | `2708:0008`, device release `0x0112` | Probe the meter block via `probes_meter_block_harmlessly` (`GET_MEM` on `0x3c`, no writes) | Passed; the device answered a whole 32-byte block. Level accuracy is unconfirmed — no input source was connected, so the bytes were not checked against a known signal |
| 2026-09-12 | iD14 MKII | `2708:0008`, device release `0x0112` | Launch the Selah GUI with the device attached (nothing connected to inputs or outputs), quit after ~25 s of live 10 Hz feedback polling | Passed; the connect refresh confirmed the speaker level plus all five monitor toggles, polling ran warning- and error-free with no repeated confirmations once steady, and quitting released the control interface. Audio interfaces 0–2 stayed bound to `snd-usb-audio` and PipeWire kept exposing the iD14 throughout |
| 2026-09-13 | iD14 MKII | `2708:0008`, device release `0x0112` | Re-ran `opens_and_closes_selected_safe_interface`, `reads_monitor_volume_harmlessly`, `probes_meter_block_harmlessly` on current tree (no writes except the session claim) | Passed 3/3; control interface 4 (`0xfe` app-specific) claimed and released, audio interfaces 0–2 stayed bound to `snd-usb-audio`, PipeWire kept exposing the iD14 |

The device release comes from the USB descriptor and is not confirmed to be the user-facing firmware version.
