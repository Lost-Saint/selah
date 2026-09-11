# Selah

Selah is an early-stage Linux control panel for Audient iD audio interfaces, written in Rust with [Iced](https://iced.rs/) and [`nusb`](https://crates.io/crates/nusb). It builds on the device and protocol research in [MixiD](https://github.com/TheOnlyJoey/MixiD).

The current application performs USB descriptor discovery, automatically refreshes when USB devices connect or disconnect, identifies known iD models, and reports unknown Audient interfaces. It can also send reference-derived speaker and headphone volume from the UI through serialized background tasks that briefly claim a safe control interface per send, then release it. There is no state readback: each slider shows the last requested level, not confirmed device state.

Transfer acceptance for both volumes was verified on an iD14 MKII without disturbing Linux audio, but the audible change is unconfirmed (headphones-only setup; the phones appear routed to a fixed-level feed, not Main Mix). See [`docs/protocol.md`](docs/protocol.md). Treat both mappings as reference-derived.

See the [roadmap](ROADMAP.md) for the path from safe device sessions to MixiD parity and a daily-driver release.

## Requirements

- The latest stable Rust toolchain, installed with [rustup](https://rustup.rs/)
- Linux development packages required by Iced's windowing backend
- An Audient iD interface for hardware testing; unit tests do not require one

## Develop

```sh
cargo run
```

Before submitting a change:

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

`Cargo.lock` is intentionally committed because Selah is an application.

The physical session check is ignored during normal test runs because it claims a USB interface. Run it only for a connected device whose four-digit product ID you have verified:

```sh
SELAH_HARDWARE_PRODUCT_ID=0008 cargo test --test hardware_session -- --ignored --exact opens_and_closes_selected_safe_interface
```

This check claims and releases only the discovered non-audio control interface. It does not send a mixer control payload. Never run it as root.

## USB permissions

Do not run Selah as root. A sample udev rule is available at [`resources/udev/70-selah.rules`](resources/udev/70-selah.rules). Review it, install it using your distribution's normal process, then reload udev rules and reconnect the interface.

The rule grants the active desktop user access to Audient USB devices. USB control is still under development, so installing it is not needed for read-only discovery or to launch the current app.

## Reference project

MixiD is the behavioral reference for known device IDs, channel capabilities, and control requests. Keep protocol findings attributable and verify changes against hardware whenever possible; compiling alone does not establish device support.
