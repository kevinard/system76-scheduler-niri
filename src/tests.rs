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
fn focused_pid_tracks_initial_new_changed_and_closed_windows() {
    let mut windows = WindowsState::default();
    for (event, expected) in [
        (
            Event::WindowsChanged {
                windows: vec![window(1, Some(101), true)],
            },
            101,
        ),
        (
            Event::WindowOpenedOrChanged {
                window: window(2, Some(202), true),
            },
            202,
        ),
        (
            Event::WindowOpenedOrChanged {
                window: window(2, Some(203), true),
            },
            203,
        ),
        (Event::WindowFocusChanged { id: Some(1) }, 101),
        (Event::WindowClosed { id: 1 }, std::process::id()),
    ] {
        windows.apply(event);
        assert_eq!(foreground_pid(&windows), expected);
    }
    assert!(!windows.windows.contains_key(&1));
}

#[test]
fn missing_focus_or_invalid_pid_clears_previous_preference() {
    let mut windows = WindowsState::default();
    windows.apply(Event::WindowsChanged {
        windows: vec![window(1, Some(101), true)],
    });
    for id in [None, Some(99)] {
        windows.apply(Event::WindowFocusChanged { id });
        assert_eq!(foreground_pid(&windows), std::process::id());
    }
    for pid in [None, Some(0), Some(-1)] {
        windows.apply(Event::WindowOpenedOrChanged {
            window: window(2, pid, true),
        });
        assert_eq!(foreground_pid(&windows), std::process::id());
    }
}

#[test]
fn replacing_snapshot_drops_old_focus() {
    let mut windows = WindowsState::default();
    windows.apply(Event::WindowsChanged {
        windows: vec![window(1, Some(101), true)],
    });
    windows.apply(Event::WindowsChanged {
        windows: vec![window(2, Some(202), false)],
    });
    assert_eq!(foreground_pid(&windows), std::process::id());
    assert_eq!(windows.windows.len(), 1);
}
