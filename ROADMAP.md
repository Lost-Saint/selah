# Selah roadmap

Selah's goal is to become a safe, native Linux control panel for Audient iD interfaces: first reaching reliable feature parity with [MixiD](https://github.com/TheOnlyJoey/MixiD), then improving the experience where a Rust and Iced implementation can do better.

This roadmap describes product outcomes, not implementation tasks or promised dates. Work moves forward only when the previous hardware boundary is safe enough to build on.

## Product promise

A user should be able to connect a supported Audient iD interface, understand its state, adjust its mixer and monitoring controls, and close Selah without interrupting or damaging the normal Linux audio session.

Selah should eventually provide:

- automatic discovery of supported Audient iD interfaces;
- mixer controls shaped by the connected model's real capabilities;
- monitor, headphone, channel, and routing controls;
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

Selah has the application foundation and the first read-only hardware slice:

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

The app can send reference-derived speaker volume from the UI through a serialized background task that claims and releases the control interface per send, with pending, sent, and failed states and no state readback. The volume mapping has not been hardware-verified yet: the opt-in `sends_harmless_speaker_volume_request` check is still awaiting a physical run, so Milestone 1 is not closed. A safe claim-and-release cycle has been physically verified on an iD14 MKII without disturbing its Linux audio interfaces; broader connect, disconnect, shutdown, and failure testing is still required.

## Milestone 1 — Safe device session

**Outcome:** Selah can establish and close a control session on one physical interface without interrupting Linux audio.

- Detect hot-plug and removal instead of relying only on manual scans. ✅
- Inspect USB interfaces and prefer the spare DFU or vendor interface identified by [MixiD issue #15](https://github.com/TheOnlyJoey/MixiD/issues/15). ✅
- Give the device session one owner with deterministic claim, release, and shutdown behavior. ✅
- Keep device I/O serialized and away from Iced's UI thread. ✅
- Distinguish permission denied, device busy, disconnected, and unsupported-interface errors. ✅
- Introduce a mockable transport boundary and test acquisition and cleanup failures. ✅
- Send one harmless, bounded control request on the maintainer's first test model.

**Exit condition:** repeated connect, disconnect, application-close, and forced-error tests leave PipeWire or ALSA audio working normally on the first target model.

## Milestone 2 — Essential monitoring

**Outcome:** one verified model can perform the daily monitor operations already available in MixiD.

- Main speaker volume.
- Headphone volume.
- Dim, mute, mono, alternate-speaker, and talkback controls where supported.
- Honest pending, applied, failed, and unknown states for each control.
- Capability-driven UI that hides controls a model does not have.
- Keyboard-accessible controls and sensible fine adjustment.

MixiD does not yet provide complete state readback, so Selah must not present a locally remembered value as confirmed device state.

**Exit condition:** every displayed monitor control works repeatedly on the first target model, failures are visible and recoverable, and restarting Selah never invents a confirmed value.

## Milestone 3 — Input mixer

**Outcome:** users can build and adjust an input mix across the connected model's analog and digital inputs.

- Model-driven channel strips for microphone and digital inputs.
- Channel level and polarity controls.
- Stereo linking, tracked upstream in [MixiD issue #22](https://github.com/TheOnlyJoey/MixiD/issues/22).
- Channel mute and solo with explicit restore semantics, informed by [MixiD issue #4](https://github.com/TheOnlyJoey/MixiD/issues/4).
- Clear naming and grouping for large ADAT channel counts.
- Horizontal navigation that remains responsive on high-channel-count models.

**Exit condition:** the mixer scales from iD4-class devices through an expanded interface without incorrect indexes, hidden channels, or stale local state.

## Milestone 4 — Routing and extended outputs

**Outcome:** users can understand, change, and undo signal routing without guessing what a grid cell means.

- Main Mix, Alt Speaker, Cue A, Cue B, and DAW Mix destinations where supported.
- A clear route-off or reset action instead of a one-way selection, addressing [MixiD issue #11](https://github.com/TheOnlyJoey/MixiD/issues/11).
- Output 3/4 and digital-output behavior, tracked in [MixiD issue #3](https://github.com/TheOnlyJoey/MixiD/issues/3).
- Correct ADAT expansion mapping, including the iD22 report in [MixiD issue #25](https://github.com/TheOnlyJoey/MixiD/issues/25).
- Insert and send/return behavior only after it is verified on capable hardware; see [MixiD issue #5](https://github.com/TheOnlyJoey/MixiD/issues/5).
- Routing layouts derived from model capabilities rather than fixed six-channel tables.

**Exit condition:** every available source and destination can be selected and cleared, and routing is verified on at least one compact interface and one digitally expanded interface.

## Milestone 5 — Device feedback and metering

**Outcome:** Selah reflects what the hardware is doing instead of acting as a write-only remote.

- Read back mixer and monitor state where protocol support is known.
- Reconcile external hardware changes without jumping or overwriting them.
- Add bounded, event-driven VU meters after the protocol is verified; upstream research is tracked in [MixiD issue #7](https://github.com/TheOnlyJoey/MixiD/issues/7).
- Suspend metering when hidden, disconnected, or unchanged to avoid continuous GPU and USB load.
- Show unavailable feedback as unknown rather than zero.

**Exit condition:** UI values survive reconnect accurately when the device supports readback, and metering remains responsive without busy polling or continuous repainting.

## Milestone 6 — Broad iD verification

**Outcome:** support claims are backed by a maintained compatibility matrix rather than inferred from similar devices.

- Verify every cataloged model with community-provided hardware results.
- Record working controls, firmware information, limitations, and regressions per model.
- Resolve model-specific counts and mappings, including ongoing iD48 investigation in [MixiD issue #1](https://github.com/TheOnlyJoey/MixiD/issues/1).
- Handle multiple connected Audient interfaces explicitly.
- Provide useful diagnostics that users can share without exposing serial numbers or full USB captures.

**Exit condition:** each advertised model has a published verification record, and partially supported models are labeled precisely.

## Milestone 7 — Daily-driver release

**Outcome:** Linux users can install, update, and rely on Selah without a development environment.

- Package the binary, desktop entry, icons, and narrowly scoped udev rule.
- Choose initial distribution formats based on real user demand rather than supporting every format at once.
- Persist user preferences and optional layouts without confusing them with confirmed hardware state.
- Recover cleanly from sleep, USB reconnects, PipeWire or ALSA restarts, and application upgrades.
- Provide accessible focus, keyboard operation, readable scaling, and reduced-motion behavior.
- Document installation, permissions, supported hardware, troubleshooting, and how to contribute verification results.

**Exit condition:** a user on a documented supported distribution can install Selah, complete the mixer workflow on a verified model, update it, and remove it cleanly.

## Beyond MixiD parity

These are candidates after the core iD experience is reliable:

- named mixer snapshots and safe A/B recall;
- per-model default layouts;
- import and export of non-sensitive configuration;
- opt-in system tray controls;
- richer diagnostics and a guided hardware-verification report;
- support for additional Audient families.

EVO support is explicitly out of scope until the iD protocol and product experience are stable. Firmware updates, DAW plug-ins, network control, and Windows or macOS releases are not current goals.

## Immediate goal

The current target is **Milestone 1: Safe device session**, using the iD14 MKII as the initial reference device. The architecture must remain capability-driven and avoid baking in its channel layout. The next software boundary is serialized device I/O; any state-changing control request remains an explicit hardware-verification step.

Roadmap priorities may change when hardware evidence disproves an assumption. Safety, honest state, and normal audio continuity take priority over feature count.
