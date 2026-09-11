# Selah

Selah is an early-stage Linux control panel for Audient iD audio interfaces, written in Rust with [Iced](https://iced.rs/) and [`nusb`](https://crates.io/crates/nusb). It builds on the device and protocol research in [MixiD](https://github.com/TheOnlyJoey/MixiD).

Selah is not ready to control hardware yet. The current application performs read-only USB discovery, automatically refreshes when USB devices connect or disconnect, identifies known iD models, and reports unknown Audient interfaces without opening the device or claiming an interface.

The device layer can identify and own a safe application/vendor control interface, but that session is not connected to the UI yet and no mixer request is sent.

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
