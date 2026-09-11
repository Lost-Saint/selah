# Selah

Selah is an early-stage Rust desktop control panel for Audient iD audio interfaces on Linux. It uses Iced for the UI and `nusb` for USB access. [MixiD](https://github.com/TheOnlyJoey/MixiD) is the primary behavioral and protocol reference, but Selah is a Rust implementation rather than a line-for-line port.

## Priorities

1. **Protect the user's audio session.** Prefer a spare DFU/vendor interface when a device exposes one. If code detaches a kernel driver or claims an interface, every error and shutdown path must release it and restore the prior state.
2. **Keep the UI responsive.** Device discovery, USB I/O, configuration I/O, and retries must not block Iced's UI thread. Avoid continuous repainting when state has not changed.
3. **Make hardware support explicit.** Put model-specific IDs and capabilities in `src/device/catalog.rs`. Do not scatter USB IDs or channel counts through UI or transport code.
4. **Keep the model small.** This is one application crate. Do not introduce a workspace, plugin framework, service locator, or extra abstraction until a concrete need exists.
5. **Fail safely.** A disconnected, unsupported, or permission-denied device should produce a useful error and leave the interface usable by the kernel audio driver.

## Repository layout

- `src/main.rs` — thin process entry point and tracing setup
- `src/lib.rs` — library boundary used by the binary and tests
- `src/app.rs` — Iced application state and views
- `src/device/` — supported-device catalog; later USB discovery and protocol code belong here
- `resources/udev/` — Linux device-access rules users may install explicitly
- `docs/` — protocol findings and durable design decisions that code cannot explain locally
- `.github/workflows/` — continuous integration

Keep pure protocol encoding/decoding separate from live USB operations. Pure code should accept values and return bytes or typed results; the USB boundary owns device handles and side effects.

## Development commands

Run the smallest relevant checks while working. Before handing off a normal code change, run:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Use `cargo run` for a local launch and `RUST_LOG=debug cargo run` when USB or startup logging is relevant. Do not run the application as root. Install the rule in `resources/udev/` when hardware access is needed.

Hardware tests are manual and opt-in. CI and the default test suite must work without an Audient device attached. Test device-independent protocol behavior with unit tests and test doubles; never make CI probe or claim arbitrary USB devices.

## Rust and Cargo conventions

- The minimum supported Rust version is the `rust-version` in `Cargo.toml`; do not use newer language or library features without raising it deliberately.
- Cargo is configured to prefer dependency releases compatible with that minimum version. Do not bypass the resolver by casually forcing a newer transitive release.
- Commit `Cargo.lock`: Selah is an application, and reproducible dependency resolution matters.
- Put project-wide Rust and Clippy lint policy in `Cargo.toml`. `clippy.toml` is only for Clippy configuration values, not `#![warn(...)]` attributes.
- Add dependencies only for current code. Disable unnecessary crate features when that materially reduces compile time or runtime cost.
- Keep optional platform integrations behind Cargo features, and document any native packages they require.
- Use `thiserror` for typed library/domain errors and `anyhow` only at application boundaries where extra context is more useful than matching an error variant.
- Prefer ownership and message passing over shared mutable global state. Unsafe Rust is forbidden unless a maintainer explicitly approves a narrowly documented exception.
- Use `tracing`, not `println!`, for diagnostics. Never log raw device buffers at info level or emit a message continuously from a refresh loop.
- Public names should describe the audio domain (`DeviceModel`, `output_channels`), not the shape of the old C++ implementation.

## Device and protocol work

- Audient's USB vendor ID is `0x2708`. Product IDs and capabilities must have a source and a focused test.
- Check the current MixiD support list and source before adding a model. Hardware facts can change as devices are released.
- Preserve attribution when behavior or protocol knowledge comes from MixiD. Implement it idiomatically in Rust rather than copying C++ structure.
- Validate all channel indexes, ranges, request values, and buffer lengths before USB I/O. Conversion code should define boundary behavior and test minimum, maximum, and invalid values.
- A discovery failure is not a fatal process error. The app should remain open and offer a retry path.

## Change discipline

- Keep changes focused and explain any hardware assumptions in the handoff.
- Add tests for catalog, configuration, and protocol behavior. UI tests should assert user-visible behavior rather than widget implementation details.
- Update `README.md` when setup or user workflow changes. Update `docs/protocol.md` when verified protocol knowledge changes.
- Do not claim support based only on compilation. State whether a change was unit-tested, launched without hardware, or verified on a named physical interface.
- Never commit captured USB traffic, local config, logs, build output, secrets, or editor state.
