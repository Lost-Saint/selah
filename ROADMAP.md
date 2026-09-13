# Selah roadmap

Selah's goal is to become a safe, native Linux control panel for Audient iD interfaces: first reaching reliable feature parity with [MixiD](https://github.com/TheOnlyJoey/MixiD), then improving the experience where a Rust and Iced implementation can do better.

This roadmap describes product outcomes, not implementation tasks or promised dates. Work moves forward only when the previous hardware boundary is safe enough to build on.

## Product promise

A user should be able to connect a supported Audient iD interface, understand its state, adjust its mixer and monitoring controls, and close Selah without interrupting or damaging the normal Linux audio session.

Selah should eventually provide:

- automatic discovery of supported Audient iD interfaces;
- mixer controls shaped by the connected model's real capabilities;
- monitor, channel, and routing controls (headphone routing included; there is
  deliberately no headphone volume slider — the official application exposes
  none);
- trustworthy device feedback and level metering where the protocol allows it;
- safe disconnect and recovery behavior;
- installation packages with narrowly scoped USB permissions;
- clear reporting of verified, partial, and unknown hardware support.

## What “supported” means

A model is **cataloged** when Selah knows its USB product ID and expected channel counts. Catalog presence alone does not mean the device is supported.

A model is **supported** only when:

1. Selah discovers it automatically.
2. Selah can use a control interface without disrupting normal audio playback or capture.
3. Every control shown for that model has a verified mapping.
4. Disconnect, application shutdown, and failed requests release all resources safely.
5. The primary workflow has been tested on physical hardware, with the model and result recorded.

Features may be marked **reference-derived** until verified on Selah hardware. The [MixiD support list](https://github.com/TheOnlyJoey/MixiD/wiki/Support-List) is useful evidence, but it does not replace testing the Rust implementation.

## Current state

Selah has the application foundation with the Milestone 2 monitor slice and the Milestone 3 input mixer:

- Iced application shell and structured logging;
- catalog entries for the nine iD models known to MixiD;
- asynchronous USB descriptor discovery;
- recognized, unsupported, absent, scanning, and failure states;
- manual rescan without opening or claiming the device;
- event-driven refresh when USB devices connect or disconnect;
- safe control-interface detection that prefers application/DFU interfaces and refuses audio interfaces;
- an explicit device-session primitive that opens and claims only a safe control interface, with drop-based and awaited release paths;
- hardware-independent catalog, discovery, and session-lifecycle tests;
- CI, formatting, lint, and dependency-maintenance configuration;
- a scoped Linux udev rule.

The app sends monitor, mixer, and capability-gated routing controls through background tasks that claim and release the control interface per operation, with pending, sent, confirmed, failed, and unknown states. Monitor volume, monitor toggles, and the iD24 optical-output mode read back through bounded `GET_CUR` polls and adopt hardware state (front-panel moves and reconnects appear instead of being overwritten); per-input VU meters read a probed `GET_MEM` block at 10 Hz with the monitor snapshot folded in every ~1 s, on one session per tick and no timer when no supported device is present. Routing, mixer levels, polarity, and headphone level stay write-only because their reads alias or stall, and show last-sent or unknown — never confirmed. Routing names physical output destinations and mix sources, supports per-output reset, and never derives a write from channel count alone. The input mixer remains one strip per microphone then digital input, with explicit ADAT numbering after onboard inputs. Channel mute, solo, stereo linking, and insert/send-return have no known USB mapping, so no working control is shown. Existing control transfers were verified on an iD14 MKII without disturbing Linux audio, but Selah's expanded routing writes still await the hardware checks recorded under Milestone 4. The new readback paths have passed transfer-level on the same model (monitor-volume roundtrip, whole meter block, and a warning-free GUI feedback session); meter level accuracy against a known signal and audible confirmation remain open. A safe claim-and-release cycle, repeated session/volume cycles, a busy-interface forced-error probe, GUI close, and unplug/replug recovery have all left Linux audio working. Hardware permission-denied handling remains unit-tested only because exercising it requires system ACL or udev changes.

## Milestone 1 — Safe device session

**Outcome:** Selah can establish and close a control session on one physical interface without interrupting Linux audio.

- Detect hot-plug and removal instead of relying only on manual scans. ✅
- Inspect USB interfaces and prefer the spare DFU or vendor interface identified by [MixiD issue #15](https://github.com/TheOnlyJoey/MixiD/issues/15). ✅
- Give the device session one owner with deterministic claim, release, and shutdown behavior. ✅
- Keep device I/O serialized and away from Iced's UI thread. ✅
- Distinguish permission denied, device busy, disconnected, and unsupported-interface errors. ✅
- Introduce a mockable transport boundary and test acquisition and cleanup failures. ✅
- Send one harmless, bounded control request on the maintainer's first test model. ✅

**Exit condition:** repeated connect, disconnect, application-close, and forced-error tests leave PipeWire or ALSA audio working normally on the first target model. ✅ (verified 2026-09-11 on the iD14 MKII; hardware permission-denied is the documented exception above)

## Milestone 2 — Essential monitoring

**Outcome:** one verified model can perform the daily monitor operations already available in MixiD.

- Main speaker volume. ✅
- Headphone volume mapping. ✅ at transfer level (no UI slider: the official
  application exposes no in-app headphone gain, and the mapping is write-only;
  see `docs/protocol.md`)
- Dim, mute, mono, alternate-speaker, and talkback controls where supported. ✅ (iD14 MKII gate; per-model differences unverified)
- Honest pending, applied, failed, and unknown states for each control. ✅
- Capability-driven UI that hides controls a model does not have. ✅
- Keyboard-accessible controls and sensible fine adjustment. ✅ (toggles are focusable buttons; sliders step 0.01)

MixiD does not yet provide complete state readback, so Selah must not present a locally remembered value as confirmed device state.

**Exit condition:** every displayed monitor control works repeatedly on the first target model, failures are visible and recoverable, and restarting Selah never invents a confirmed value. ✅ (code-complete with transfer tests; audible listening confirmation is the documented exception above)

## Milestone 3 — Input mixer

**Outcome:** users can build and adjust an input mix across the connected model's analog and digital inputs.

- Model-driven channel strips for microphone and digital inputs. ✅ (counts from the catalog; iD14 MKII gate)
- Channel level and polarity controls. ✅
- Stereo linking, tracked upstream in [MixiD issue #22](https://github.com/TheOnlyJoey/MixiD/issues/22). ➖ (no known USB mapping in MixiD, BiD, or Monix; no control shown)
- Channel mute and solo with explicit restore semantics, informed by [MixiD issue #4](https://github.com/TheOnlyJoey/MixiD/issues/4). ➖ (no known USB mapping; no control shown)
- Clear naming and grouping for large ADAT channel counts. ✅ (Mic N / Digi N running index)
- Horizontal navigation that remains responsive on high-channel-count models. ✅ (scrollable strip row; no polling or repaint loops)

**Exit condition:** the mixer scales from iD4-class devices through an expanded interface without incorrect indexes, hidden channels, or stale local state. ✅ (code-complete with boundary tests; audible listening confirmation is the documented exception above)

## Milestone 4 — Routing and extended outputs

**Outcome:** users can understand, change, and undo signal routing without guessing what a grid cell means.

- Main Mix, Alt Speaker, Cue A, Cue B, and DAW Mix sources where supported. ✅ (iD14 MKII and iD24 reference-derived maps; no unsupported choices shown)
- A clear route-off or reset action instead of a one-way selection, addressing [MixiD issue #11](https://github.com/TheOnlyJoey/MixiD/issues/11). ✅ (the protocol has no off state; Reset sends the destination's documented default)
- Output 3/4 and digital-output behavior, tracked in [MixiD issue #3](https://github.com/TheOnlyJoey/MixiD/issues/3). ✅ (outputs 3/4 routing on both enabled profiles; evidenced iD24 ADAT/S/PDIF output format)
- Correct ADAT expansion mapping, including the iD22 report in [MixiD issue #25](https://github.com/TheOnlyJoey/MixiD/issues/25). ✅ (ADAT 1–8 follow onboard inputs; output routing indexes are modeled separately)
- Insert and send/return behavior only after it is verified on capable hardware; see [MixiD issue #5](https://github.com/TheOnlyJoey/MixiD/issues/5). ➖ (no verified mapping; capability remains visibly unavailable and sends nothing)
- Routing layouts derived from model capabilities rather than fixed six-channel tables. ✅

**Exit condition:** every available source and destination can be selected and reset. ✅ in code and hardware-independent tests. Source mappings are externally evidenced on iD14 MKII and iD24; Selah's own audible confirmation is best-effort on the maintainer's iD14 MKII and does not block close-out. Unverified models stay cataloged with no routing map exposed.

## Milestone 5 — Device feedback and metering

**Outcome:** Selah reflects what the hardware is doing instead of acting as a write-only remote.

- Read back mixer and monitor state where protocol support is known. ✅ in code (monitor volume, monitor toggles, and optical-output mode via BiD-evidenced `GET_CUR`; mixer levels, polarity, routing, and headphone level are evidenced write-only, so they stay last-sent/unknown rather than faked)
- Reconcile external hardware changes without jumping or overwriting them. ✅ in code (confirmed state wins; pending sends finish before adoption; reconnect resets to unknown before readback)
- Add bounded, event-driven VU meters after the protocol is verified; upstream research is tracked in [MixiD issue #7](https://github.com/TheOnlyJoey/MixiD/issues/7). ✅ in code (probed `GET_MEM` block at 10 Hz on one session per tick, capability-gated to models with mixer strips; short blocks rejected, failures blank instead of freezing)
- Suspend metering when hidden, disconnected, or unchanged to avoid continuous GPU and USB load. ✅ in code (the timer exists only while a readable device is present; mid-poll ticks are dropped; unchanged values stay quiet — with the note that the current single-window app has no meaningful hidden state to suspend on)
- Show unavailable feedback as unknown rather than zero. ✅ in code (unknown meters render as a placeholder, never a zero bar; unconfirmed levels never default to `0`, `false`, or the last sent value)

**Exit condition:** UI values survive reconnect accurately when the device supports readback, and metering remains responsive without busy polling or continuous repainting. ✅ in code and hardware-independent tests, with transfer-level evidence on an iD14 MKII (monitor roundtrip, meter block, warning-free GUI session). Meter accuracy against a known signal and audible confirmation are best-effort follow-ups recorded in `docs/protocol.md`, not close-out blockers.

## Milestone 6 — Broad iD verification

**Outcome:** support claims are backed by a maintained compatibility matrix rather than inferred from similar devices. The maintainer verifies on an iD14 MKII only; every other model depends on community-provided hardware results.

- Verify the iD14 MKII as the primary target with maintainer hardware results.
- Accept community results for every other cataloged model; unverified models stay cataloged with unavailable controls hidden, never claimed as supported.
- Record working controls, firmware information, limitations, and regressions per model.
- Resolve model-specific counts and mappings, including ongoing iD48 investigation in [MixiD issue #1](https://github.com/TheOnlyJoey/MixiD/issues/1).
- Handle multiple connected Audient interfaces explicitly.
- Provide useful diagnostics that users can share without exposing serial numbers or full USB captures.

**Exit condition:** the iD14 MKII has a published verification record; each additional advertised model gains one only after its own hardware result, and partially supported models are labeled precisely.

## Milestone 7 — Daily-driver release

**Outcome:** Linux users can install, update, and rely on Selah without a development environment.

- Package the binary, desktop entry, icons, and narrowly scoped udev rule.
- Choose initial distribution formats based on real user demand rather than supporting every format at once.
- Persist user preferences and optional layouts without confusing them with confirmed hardware state.
- Recover cleanly from sleep, USB reconnects, PipeWire or ALSA restarts, and application upgrades.
- Provide accessible focus, keyboard operation, readable scaling, and reduced-motion behavior.
- Document installation, permissions, supported hardware, troubleshooting, and how to contribute verification results.

**Exit condition:** a user on a documented supported distribution can install Selah, complete the mixer workflow on a verified model, update it, and remove it cleanly.

## Audient-verified iD Mixer backlog

The features below are documented Audient iD Mixer functionality, confirmed
against Audient's Help Desk on 2026-09-13 ("What do Main, Cue and DAW Thru
do?" and "ID14 Loop-back Setup").
That confirms what the official mixer *does* — it is documentation evidence,
not USB-mapping evidence. A backlog item becomes milestone work only when its
USB mapping is evidenced from MixiD, BiD, hardware capture, or Selah's own
listening tests; Audient docs alone never enable a write, and per-model
differences (iD4 through iD44) each need their own mapping.

Already in Selah (reference-derived or verified, model-gated as noted):

- analogue (mic/line/DI) and digital (optical ADAT/S/PDIF) input channels
  with faders, meters, and polarity;
- Main Mix, Cue A/B, and DAW Mix/DAW Thru routing to physical outputs, with
  reset-to-documented-default instead of an invented off state;
- talkback, mono, dim, mute, and alternate-speaker monitor toggles on the
  iD14 MKII gate;
- channel visibility for analogue and digital groups (DAW returns unmapped).

Needs a USB mapping before any control is shown:

- per-channel mute, solo, and pan;
- stereo linking of adjacent channels;
- mix-specific faders, cue master level, cue solo, and main/cue meters;
- monitor cut and adjustable dim level;
- DAW return channels as mixer strips;
- talkback source selection (input or computer audio device, per interface);
- loopback source selection (iD14 MKII sources are documented as DAW 1+2,
  3+4, 5+6, Master Mix, Cue A, Cue B; the original iD14 MKI has no loopback
  hardware);
- mixer presets: save, load, export, and import;
- channel renaming.

App-side work needing no USB mapping:

- keyboard shortcuts beyond the current focusable buttons and 0.01 slider
  steps;
- ScrollControl-style encoder behavior where the hardware supports it.

Explicitly not iD Mixer features — Selah will not invent them:

- built-in channel EQ, compressor, reverb, delay, or noise gate;
- arbitrary VST/plugin hosting inside the mixer.

(Audient's reverb-tracking documentation describes DAW plugins, not mixer
DSP.)

## Beyond MixiD parity

These are candidates after the core iD experience is reliable (mixer presets
and configuration import/export now live in the backlog above with their
evidence gates):

- per-model default layouts;
- opt-in system tray controls;
- richer diagnostics and a guided hardware-verification report;
- support for additional Audient families.

EVO support is explicitly out of scope until the iD protocol and product experience are stable. Firmware updates, DAW plug-ins, network control, and Windows or macOS releases are not current goals.

## Immediate goal

The current target is **Milestone 6 on a single-device budget**: keep the iD14 MKII as the verified primary, keep every other model cataloged-but-unverified with unavailable controls hidden, and grow the compatibility matrix only from community hardware results. Audible and meter-accuracy checks on the iD14 MKII are best-effort and recorded in `docs/protocol.md` when they happen.

Roadmap priorities may change when hardware evidence disproves an assumption. Safety, honest state, and normal audio continuity take priority over feature count.
