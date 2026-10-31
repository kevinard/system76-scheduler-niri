# system76-scheduler-niri

A simple daemon to update the foreground process of [system76-scheduler](https://github.com/pop-os/system76-scheduler)
based on the focused window in Niri.

## Installation

The program can be installed to `~/.cargo/bin` by running `cargo install --path .` in the
cloned repository.

### NixOS

A Nix flake is provided that contains the package, and a defined systemd user service
as a home manager module.

**Note**: system76-scheduler has to be enabled separately with `services.system76-scheduler.enable`.

## Usage

Simply start `system76-scheduler-niri` when Niri starts. The system76-scheduler
daemon must be started separately; this helper does not activate it over D-Bus.

Window state is maintained with niri-ipc's `WindowsState`, including the initial
focused window, newly opened or changed windows, closed windows, and focus loss.
When focus or a valid PID is unavailable, the helper selects its own PID so the
previous application does not retain the foreground preference. Window titles
are not logged.

The current selection is resent every ten seconds to recover after a scheduler
daemon restart. Failed D-Bus calls retry after three seconds. An event-stream
error exits with a failure status so a service manager can restart the helper.

The Rust toolchain is recorded in `rust-toolchain.toml`; rustup installs it when
needed. The IPC dependency is pinned to Niri 26.04's API. Review this pin when updating
Niri: new IPC event variants require a matching client library.

Run `cargo test --locked`, `cargo fmt --check`, and
`cargo clippy --locked --all-targets -- -D warnings` to validate changes.
The integration test requires `dbus-daemon` and uses a private bus and a fake
Niri socket; it never connects to the running desktop.
