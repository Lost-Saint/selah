# Selah

Selah is an early-stage Linux desktop control panel for Audient iD audio interfaces. It is written in Rust, uses Iced for the UI, and uses `nusb` at the hardware boundary.

[MixiD](https://github.com/TheOnlyJoey/MixiD) is our primary behavioral and protocol reference. Selah is not a line-for-line port: protocol knowledge carries forward, while the implementation should be safe, idiomatic Rust.

## What makes Selah special?

Selah exists because Audient does not provide a Linux control panel for the iD series. Users should not have to give up mixer controls, compromise their audio setup, or run an opaque binary just because they use Linux.

### 1. Open at the core

The device protocol is not officially documented. Keep our protocol findings, supported-device data, limitations, and source code open so users can verify them, contribute hardware results, and maintain forks.

Preserve attribution when knowledge comes from MixiD or another project. Do not copy implementation structure merely because it already exists; understand the behavior and express it clearly in Rust.

### 2. Protect the audio session

The control panel must not casually interrupt playback or leave an interface detached from the kernel audio driver. Prefer a spare DFU or vendor interface when a device exposes one. Any fallback that detaches a kernel driver must restore it on every error, disconnect, and shutdown path.

Never treat cleanup as optional. A UI error is recoverable; leaving the user's audio interface in a broken state is not.

### 3. Performance without compromise

Audio tools stay open for long sessions. Selah should remain quiet when nothing changes: no continuously repainting UI, busy polling, unbounded logging, or blocking USB work on Iced's UI thread.

Measure before adding caches, background workers, or complicated state management. Use the smallest design that keeps input, rendering, discovery, and device I/O responsive.

### 4. Hardware truth over optimistic claims

Compiling is not hardware verification. A product ID in the catalog means the model is known; it does not prove that every control works. State clearly whether behavior is based on a reference implementation, a unit test, or a named physical device and firmware version.

## A note from Lost Saint

I like ambitious ideas, simple systems, and software that feels obvious. Do not preserve complexity just because it already exists. Do not introduce machinery because it looks architecturally impressive. Understand the real constraint, then fight for the smallest model that makes the correct behavior unsurprising.

Channel both "measure twice, cut once" and "yagni". Fight scope creep. Try to honor the developer's intent in both a minimal and realistic fashion.

The rest of this document is meant to help you navigate the codebase and make changes effectively. Treat these instructions as strong defaults. A direct request from the maintainer may override them.

## A small glossary

Use this language when discussing Selah:

- **you** means the agent reading this file and changing Selah.
- **we, us, and maintainers** mean Lost Saint and the people building Selah.
- **user** means the person running Selah to control an audio interface.
- **app** means the Selah desktop process and its Iced UI.
- **device** or **interface** means a physical Audient USB audio interface.
- **model** means static facts about a supported product, such as its USB product ID and channel counts.
- **catalog** means the model data in `src/device/catalog.rs`.
- **protocol** means the control requests and data exchanged with an interface.
- **transport** means the side-effecting USB layer that discovers devices, owns handles, claims interfaces, and sends protocol data.
- **control interface** means the USB interface Selah claims for mixer commands. It is not necessarily the USB AudioControl interface.
- **audio driver** means the operating system's kernel driver responsible for normal audio playback and capture.
- **hardware verification** means observing behavior on a named physical model, ideally with its firmware version recorded.

## The three ways to hurt yourself

1. **Running as root or granting the world USB access.** Do not use `sudo cargo run`, recommend `MODE="0666"`, or silently install system rules. Use the scoped rule in `resources/udev/`, let the user install it explicitly, and keep normal development hardware-independent.
2. **Claiming an interface without restoring it.** A USB handle, claimed interface, or detached kernel driver needs one clear owner and deterministic cleanup. Exercise failure paths as seriously as the successful request path.
3. **Putting hardware work on the UI thread.** Discovery, control transfers, configuration I/O, sleeps, and retry loops must not block Iced's update or view work. Send bounded results back into the app as messages.

## Hit every surface

The most likely Selah defect is a change that works for one model or one lifecycle path and is missing everywhere else. Before calling a feature done, walk this list and say which entries applied:

- **Models.** Check different input, output, digital, and insert counts. Do not design only for the device you own.
- **Lifecycle.** Consider startup with no device, connection, permission denial, disconnect during I/O, reconnect, app shutdown, and device replacement.
- **Protocol.** Validate channel indexes, numeric ranges, request values, byte order, and buffer lengths before USB I/O.
- **Transport.** Decide which USB interface is claimed, how concurrent requests are serialized, and how every acquired resource is released.
- **UI.** Provide honest disconnected, connecting, ready, unsupported, permission-denied, and recoverable-error states. A failure should offer a retry path where appropriate.
- **Configuration.** New persisted values need defaults, validation, forward-compatible loading, and a way to reset or stop using them.
- **Packaging.** Check whether Linux permissions, desktop integration, installed resources, or distribution packages are affected.
- **Docs.** Update user setup or protocol notes only when the change makes existing guidance inaccurate or adds durable verified knowledge.

## Development

- Install Rust with rustup. `rust-toolchain.toml` selects stable Rust with Clippy and rustfmt.
- `cargo run` starts the app.
- `RUST_LOG=debug cargo run` enables useful startup and transport diagnostics.
- Do not run Selah as root. Hardware access should use the rule in `resources/udev/70-selah.rules`.
- Stop processes you started by the PID you captured. Do not kill by a broad process-name pattern.
- Keep `Cargo.lock` committed. Selah is an application, and reproducible dependency resolution matters.

When adding a dependency, explain the current need. Prefer a crate's minimal feature set, check its minimum Rust version and native system requirements, and avoid adding infrastructure for hypothetical future work.

## Test hardware and protocol data

Automated tests must run without an Audient device attached. Keep hardware tests manual and opt-in unless a purpose-built mock transport is available.

- Put pure request encoding, decoding, range conversion, and catalog logic under unit tests.
- Put USB ownership and orchestration behind a boundary that can be exercised with test doubles.
- Never let the default test suite enumerate, open, claim, reset, or detach arbitrary devices on the machine running it.
- Do not commit personal logs, full USB captures, serial numbers, or machine-specific paths.
- Small sanitized byte fixtures are acceptable when their source and expected meaning are documented.
- When testing physically, record the exact model, relevant firmware version if available, result, and whether normal audio continued working.

## Verifying

Use the smallest proof that demonstrates the change, then run the normal project checks before handoff:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
```

- Test meaningful logic and observable behavior. Do not add tests that merely repeat constants or mirror an implementation line by line.
- Protocol behavior changes need focused boundary tests, including invalid and minimum/maximum inputs.
- A UI feature should be launched when practical, but do not claim hardware behavior from a launch without hardware.
- Do not require physical hardware, root access, network access, or a graphical session in CI.
- State what was not tested. "Not verified on hardware" is useful information, not a failure to hide.

## Rust and Cargo

- Keep project-wide Rust and Clippy lint levels in `Cargo.toml`. A `clippy.toml` file is only for Clippy configuration values.
- Unsafe Rust is forbidden by default. If a required native boundary cannot avoid it, stop and get maintainer approval for a narrow, documented exception.
- Prefer typed domain errors for recoverable device failures. Add context at the app boundary without erasing distinctions the UI needs.
- Use `tracing` instead of `println!` for diagnostics. Do not log raw device buffers at info level or emit unchanged state from a refresh loop.
- Prefer explicit domain names such as `DeviceModel`, `analog_outputs`, and `control_interface` over names inherited from the shape of the old C++ code.
- Keep platform-specific dependencies and behavior behind target configuration or Cargo features.
- Do not create a workspace, internal crates, build script, proc macro, or feature matrix until a concrete boundary requires one.
- Release-profile changes are product behavior. Consider startup time, binary size, crash diagnostics, and cleanup semantics before changing them.

## Pull requests

- Never create a pull request unless the maintainer explicitly asks.
- Use Conventional Commit titles in plain language, for example: `fix(usb): release the control interface on disconnect`.
- Keep one concern per pull request. If the description naturally says "also," split the work.
- Start the body with the user-visible or hardware problem, then explain the smallest solution.
- List verification precisely: automated commands, launch environment, and physical device model when applicable.
- UI changes should include before/after images. Timing, meters, or animation changes should include a short recording.
- Never commit PR-only screenshots, USB captures, logs, or scratch artifacts.
- Verify automated review findings against the source before changing code. Explain why a false positive is false.

## Documentation

Most code changes do not need new documentation. Agents can read types, tests, and local comments.

- `README.md` helps users install, run, and understand the current maturity of Selah. Keep it honest and concise.
- `ROADMAP.md` holds public product outcomes and milestone exit conditions. Keep task-level implementation notes in the issue that owns the work.
- `docs/protocol.md` holds durable protocol findings, their sources, uncertainty, and physical verification status.
- Add a nearby code comment when reasoning is local to one request, conversion, or cleanup path.
- Use a deeper internal document only when a decision crosses the app, protocol, and transport boundaries or records a trap that is difficult to discover from code.
- Do not maintain file catalogs, narrate control flow, duplicate types, or append a changelog of every implementation step.
- When a documented assumption changes, rewrite or remove the old statement instead of appending a conflicting account.
- User documentation must not claim support based only on catalog presence or compilation.

## Plans and work artifacts

- Do not commit implementation plans, research notes, generated agent summaries, or scratch files.
- Track ongoing maintainer work in the issue or discussion that owns it.
- Keep temporary upstream checkouts and protocol exploration outside the worktree.
- A merged pull request and its tests are the implementation record; do not preserve a second checklist in the repository.

## How it works

Iced owns the application state and renders views from that state. UI actions become messages. Device discovery and USB operations run away from the UI thread, then return typed results as messages.

The device catalog describes static model capabilities. Pure protocol code turns validated domain values into control requests and decodes device responses. The transport owns live `nusb` handles, selects and claims a control interface, serializes I/O, and releases resources. The UI never constructs raw USB packets or owns a device handle.

```text
Iced view -> message -> app update -> device command
                                      |
                                      v
                               protocol encoding
                                      |
                                      v
                               nusb transport
                                      |
                                      v
device result <- app message <- typed result/error
```

This is the intended boundary, not a requirement to build every layer before it is needed.

## Where code lives

- `src/main.rs` — thin process entry point and tracing setup.
- `src/lib.rs` — library boundary used by the binary and tests.
- `src/app.rs` — Iced application state, messages, update logic, and views.
- `src/device/catalog.rs` — Audient vendor/product IDs and static model capabilities.
- `src/device/` — future protocol and USB transport modules.
- `resources/udev/` — user-installed Linux device-access rules.
- `docs/protocol.md` — sourced protocol knowledge and verification notes.
- `ROADMAP.md` — product direction, scope boundaries, and milestone outcomes.
- `.github/workflows/` — continuous integration.

Keep modules cohesive. Split a file when it contains two real responsibilities, not merely because it has become a certain number of lines.

## Taste

- Complexity belongs at the hardware boundary. Protocol code stays pure and the UI stays ignorant of raw USB details.
- Make invalid states difficult to represent. Validate before side effects.
- Cleanup paths deserve the same design attention as successful paths.
- Prefer one obvious owner for device state over shared mutable globals.
- The UI should tell the truth: no lying connection indicator, frozen meter, stale device name, or control that silently failed.
- Use subscriptions and redraws only when state can actually change. Audio software may remain open all day.
- Comments explain why a hardware behavior or constraint exists. Types and function names explain what the code does.
- If a rule here fights the task in front of you, say so clearly and get maintainer sign-off before breaking it.

## Additional tips

- MixiD is a valuable reference, not an infallible specification. Confirm surprising behavior against source history, issues, and hardware.
- Security matters most at the USB and permissions boundaries. Do not over-engineer unrelated application code in the name of security.
- Do not install udev rules, system packages, or Rust toolchains globally unless the maintainer explicitly asks.
- Do not use a graphical session or interact with physical hardware without the maintainer's permission.
