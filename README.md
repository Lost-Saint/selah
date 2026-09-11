# Selah

Selah is an early-stage Linux control panel for Audient iD audio interfaces, written in Rust with [Iced](https://iced.rs/) and [`nusb`](https://crates.io/crates/nusb). It builds on the device and protocol research in [MixiD](https://github.com/TheOnlyJoey/MixiD).

Selah is not ready to control hardware yet. The current application provides the project foundation and a catalog of known iD devices while USB discovery and mixer controls are developed.

## Requirements

- Rust 1.88 or newer, installed with [rustup](https://rustup.rs/)
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

## USB permissions

Do not run Selah as root. A sample udev rule is available at [`resources/udev/70-selah.rules`](resources/udev/70-selah.rules). Review it, install it using your distribution's normal process, then reload udev rules and reconnect the interface.

The rule grants the active desktop user access to Audient USB devices. USB control is still under development, so installing it is not needed to launch the current app.

## Reference project

MixiD is the behavioral reference for known device IDs, channel capabilities, and control requests. Keep protocol findings attributable and verify changes against hardware whenever possible; compiling alone does not establish device support.
