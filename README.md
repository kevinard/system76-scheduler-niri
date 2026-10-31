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

An event-stream or D-Bus error exits with a failure status so a service manager
can restart the helper. It uses a single blocking event loop. If only the
scheduler daemon restarts, restart the helper to resend the unchanged focus.

The IPC dependency is pinned to Niri 26.04's API. Review it when updating Niri.
Build with your configured Rust toolchain and Cargo.lock. Run `cargo test --locked`,
`cargo fmt --check`, and `cargo clippy --locked --all-targets -- -D warnings`
to validate changes.
