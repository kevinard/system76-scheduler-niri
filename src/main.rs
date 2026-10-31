use std::io::{self, Error};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use log::{error, info};
use niri_ipc::{
    Response,
    socket::Socket,
    state::{EventStreamStatePart, WindowsState},
};
use zbus::blocking::connection;

const RESYNC_INTERVAL: Duration = Duration::from_secs(10);
const RETRY_INTERVAL: Duration = Duration::from_secs(3);

#[zbus::proxy(
    default_service = "com.system76.Scheduler",
    interface = "com.system76.Scheduler",
    default_path = "/com/system76/Scheduler"
)]
trait System76Scheduler {
    #[zbus(no_autostart)]
    fn set_foreground_process(&self, pid: u32) -> zbus::Result<()>;
}

struct Foreground {
    windows: WindowsState,
    last_pid: Option<u32>,
    next_sync: Instant,
    retry_at: Instant,
}

impl Foreground {
    fn new(now: Instant) -> Self {
        Self {
            windows: WindowsState::default(),
            last_pid: None,
            next_sync: now + RESYNC_INTERVAL,
            retry_at: now,
        }
    }

    fn pid(&self) -> u32 {
        self.windows
            .windows
            .values()
            .find(|window| window.is_focused)
            .and_then(|window| window.pid)
            .and_then(|pid| u32::try_from(pid).ok())
            .filter(|&pid| pid != 0)
            // The scheduler has no unset method. Selecting this helper ensures
            // no application keeps the old preference when focus or PID is absent.
            .unwrap_or_else(std::process::id)
    }

    fn sync<E>(
        &mut self,
        now: Instant,
        mut send: impl FnMut(u32) -> Result<(), E>,
    ) -> Result<(), E> {
        let pid = self.pid();
        if now < self.retry_at || (self.last_pid == Some(pid) && now < self.next_sync) {
            return Ok(());
        }
        match send(pid) {
            Ok(()) => {
                self.last_pid = Some(pid);
                self.next_sync = now + RESYNC_INTERVAL;
                info!("Set foreground process PID to {pid}");
                Ok(())
            }
            Err(error) => {
                self.retry_at = now + RETRY_INTERVAL;
                self.next_sync = self.retry_at;
                Err(error)
            }
        }
    }
}

fn main() -> io::Result<()> {
    colog::init();

    let mut socket = Socket::connect()?;

    let conn = connection::Builder::system()
        .map_err(Error::other)?
        .method_timeout(Duration::from_secs(2))
        .build()
        .map_err(Error::other)?;

    let proxy = System76SchedulerProxyBlocking::new(&conn).map_err(Error::other)?;

    let reply = socket.send(niri_ipc::Request::EventStream)?;

    if !matches!(reply, Ok(Response::Handled)) {
        return Err(Error::other(format!(
            "Niri didn't handle event stream request: {reply:?}"
        )));
    }
    // Keep socket reads blocking while still being able to resend after a daemon
    // restart on an otherwise idle desktop. Bound the queue if D-Bus is slow.
    let (tx, rx) = mpsc::sync_channel(16);
    std::thread::spawn(move || {
        let mut read_event = socket.read_events();
        loop {
            let event = read_event();
            let failed = event.is_err();
            if tx.send(event).is_err() || failed {
                break;
            }
        }
    });

    let mut foreground = Foreground::new(Instant::now());
    loop {
        let event = rx.recv_timeout(
            foreground
                .next_sync
                .saturating_duration_since(Instant::now()),
        );
        match event {
            Ok(Ok(event)) => {
                foreground.windows.apply(event);
            }
            Err(RecvTimeoutError::Timeout) => (),
            other => {
                let _ = proxy.set_foreground_process(std::process::id());
                return Err(match other {
                    Ok(Err(error)) => error,
                    _ => Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Niri event stream disconnected",
                    ),
                });
            }
        }
        if let Err(why) = foreground.sync(Instant::now(), |pid| proxy.set_foreground_process(pid)) {
            error!("Failed to set foreground process PID: {why}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niri_ipc::{Event, Window, WindowLayout};

    fn window(id: u64, pid: Option<i32>, is_focused: bool) -> Window {
        Window {
            id,
            pid,
            is_focused,
            title: None,
            app_id: None,
            workspace_id: None,
            is_floating: false,
            is_urgent: false,
            focus_timestamp: None,
            layout: WindowLayout {
                pos_in_scrolling_layout: None,
                tile_size: (0., 0.),
                window_size: (0, 0),
                tile_pos_in_workspace_view: None,
                window_offset_in_tile: (0., 0.),
            },
        }
    }

    #[test]
    fn snapshot_synchronizes_already_focused_window() {
        let mut foreground = Foreground::new(Instant::now());
        foreground.windows.apply(Event::WindowsChanged {
            windows: vec![window(1, Some(101), true)],
        });
        assert_eq!(foreground.pid(), 101);
    }

    #[test]
    fn new_windows_pid_changes_focus_switches_and_close_are_tracked() {
        let mut foreground = Foreground::new(Instant::now());
        foreground.windows.apply(Event::WindowsChanged {
            windows: vec![window(1, Some(101), true)],
        });
        foreground.windows.apply(Event::WindowOpenedOrChanged {
            window: window(2, Some(202), true),
        });
        assert_eq!(foreground.pid(), 202);
        foreground.windows.apply(Event::WindowOpenedOrChanged {
            window: window(2, Some(203), true),
        });
        assert_eq!(foreground.pid(), 203);
        foreground
            .windows
            .apply(Event::WindowFocusChanged { id: Some(1) });
        assert_eq!(foreground.pid(), 101);
        foreground.windows.apply(Event::WindowClosed { id: 1 });
        assert_eq!(foreground.pid(), std::process::id());
        assert!(!foreground.windows.windows.contains_key(&1));
    }

    #[test]
    fn missing_focus_unknown_window_and_invalid_pid_release_preference() {
        let mut foreground = Foreground::new(Instant::now());
        foreground.windows.apply(Event::WindowsChanged {
            windows: vec![window(1, Some(101), true)],
        });
        foreground
            .windows
            .apply(Event::WindowFocusChanged { id: None });
        assert_eq!(foreground.pid(), std::process::id());
        foreground
            .windows
            .apply(Event::WindowFocusChanged { id: Some(99) });
        assert_eq!(foreground.pid(), std::process::id());
        for pid in [None, Some(0), Some(-1)] {
            foreground.windows.apply(Event::WindowOpenedOrChanged {
                window: window(2, pid, true),
            });
            assert_eq!(foreground.pid(), std::process::id());
        }
    }

    #[test]
    fn full_snapshot_replaces_old_windows() {
        let mut foreground = Foreground::new(Instant::now());
        foreground.windows.apply(Event::WindowsChanged {
            windows: vec![window(1, Some(101), true)],
        });
        foreground.windows.apply(Event::WindowsChanged {
            windows: vec![window(2, Some(202), true)],
        });
        assert_eq!(foreground.pid(), 202);
        assert!(!foreground.windows.windows.contains_key(&1));
    }

    #[test]
    fn duplicates_are_suppressed_and_idle_resends_recover_scheduler_restart() {
        let now = Instant::now();
        let mut foreground = Foreground::new(now);
        foreground.windows.apply(Event::WindowsChanged {
            windows: vec![window(1, Some(101), true)],
        });
        let mut sent = Vec::new();
        for elapsed in [Duration::ZERO, Duration::from_secs(1), RESYNC_INTERVAL] {
            foreground
                .sync(now + elapsed, |pid| {
                    sent.push(pid);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        assert_eq!(sent, vec![101, 101]);
    }

    #[test]
    fn failed_calls_back_off_then_retry_the_latest_focus() {
        let now = Instant::now();
        let mut foreground = Foreground::new(now);
        foreground.windows.apply(Event::WindowsChanged {
            windows: vec![window(1, Some(101), true)],
        });
        assert_eq!(
            foreground.sync(now, |_| Err("unavailable")),
            Err("unavailable")
        );
        foreground.windows.apply(Event::WindowOpenedOrChanged {
            window: window(2, Some(202), true),
        });
        foreground
            .sync(now + Duration::from_secs(1), |_| -> Result<(), ()> {
                panic!("retry before backoff elapsed")
            })
            .unwrap();
        let mut sent = Vec::new();
        foreground
            .sync(now + RETRY_INTERVAL, |pid| {
                sent.push(pid);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(sent, vec![202]);
    }
}
