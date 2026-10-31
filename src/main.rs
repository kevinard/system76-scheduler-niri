use std::io::Error;

use log::info;
use niri_ipc::{
    Response,
    socket::Socket,
    state::{EventStreamStatePart, WindowsState},
};
use zbus::blocking::Connection;

#[zbus::proxy(
    default_service = "com.system76.Scheduler",
    interface = "com.system76.Scheduler",
    default_path = "/com/system76/Scheduler"
)]
trait System76Scheduler {
    #[zbus(no_autostart)]
    fn set_foreground_process(&self, pid: u32) -> zbus::Result<()>;
}

fn foreground_pid(windows: &WindowsState) -> u32 {
    windows
        .windows
        .values()
        .find(|window| window.is_focused)
        .and_then(|window| window.pid)
        .and_then(|pid| u32::try_from(pid).ok())
        .filter(|&pid| pid != 0)
        // Selecting this helper clears the previous application's preference.
        .unwrap_or_else(std::process::id)
}

fn main() -> std::io::Result<()> {
    colog::init();

    let mut socket = Socket::connect()?;
    let conn = Connection::system().map_err(Error::other)?;
    let proxy = System76SchedulerProxyBlocking::new(&conn).map_err(Error::other)?;
    let reply = socket.send(niri_ipc::Request::EventStream)?;
    if !matches!(reply, Ok(Response::Handled)) {
        return Err(Error::other(format!(
            "Niri didn't handle event stream request: {reply:?}"
        )));
    }

    let mut windows = WindowsState::default();
    let mut last_pid = None;
    let mut read_event = socket.read_events();
    loop {
        if windows.apply(read_event()?).is_some() {
            continue;
        }
        let pid = foreground_pid(&windows);
        if last_pid != Some(pid) {
            proxy.set_foreground_process(pid).map_err(Error::other)?;
            last_pid = Some(pid);
            info!("Set foreground process PID to {pid}");
        }
    }
}

#[cfg(test)]
mod tests;
