use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Directory(std::path::PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Scheduler(Sender<u32>);

#[zbus::interface(name = "com.system76.Scheduler")]
impl Scheduler {
    fn set_foreground_process(&self, pid: u32) {
        self.0.send(pid).unwrap();
    }
}

fn scheduler(address: &str) -> (zbus::blocking::Connection, Receiver<u32>) {
    let (tx, rx) = mpsc::channel();
    let connection = zbus::blocking::connection::Builder::address(address)
        .unwrap()
        .name("com.system76.Scheduler")
        .unwrap()
        .serve_at("/com/system76/Scheduler", Scheduler(tx))
        .unwrap()
        .build()
        .unwrap();
    (connection, rx)
}

#[test]
fn real_ipc_tracks_new_windows_and_recovers_after_scheduler_restart() {
    // Both endpoints are disposable: never connect to the running desktop/bus.
    let mut bus = Process(
        Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon is required for integration tests"),
    );
    let mut address = String::new();
    BufReader::new(bus.0.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let address = address.trim();
    let (service, calls) = scheduler(address);

    let directory = Directory(std::env::temp_dir().join(format!(
            "scheduler-niri-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    std::fs::create_dir(&directory.0).unwrap();
    let path = directory.0.join("niri.sock");
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut helper = Process(
        Command::new(env!("CARGO_BIN_EXE_system76-scheduler-niri"))
            .env("NIRI_SOCKET", &path)
            .env("DBUS_SYSTEM_BUS_ADDRESS", address)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let (mut stream, _) = loop {
        match listener.accept() {
            Ok(connection) => break connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "helper never connected"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = String::new();
    BufReader::new(stream.try_clone().unwrap())
        .read_line(&mut request)
        .unwrap();
    assert_eq!(request, "\"EventStream\"\n");
    stream
        .write_all(b"{\"Ok\":\"Handled\"}\n{\"CastsChanged\":{\"casts\":[]}}\n{\"WindowsChanged\":{\"windows\":[]}}\n")
        .unwrap();
    assert_eq!(
        calls.recv_timeout(Duration::from_secs(5)).unwrap(),
        helper.0.id()
    );

    let opened = serde_json::json!({"WindowOpenedOrChanged": {"window": {
        "id": 1, "pid": 123, "title": "do not log this", "app_id": "test",
        "is_focused": true, "is_floating": false, "is_urgent": false,
        "layout": {"tile_size": [100.0, 100.0], "window_size": [100, 100],
                   "window_offset_in_tile": [0.0, 0.0]}
    }}});
    writeln!(stream, "{opened}").unwrap();
    assert_eq!(calls.recv_timeout(Duration::from_secs(5)).unwrap(), 123);
    writeln!(
        stream,
        "{}",
        serde_json::json!({"WindowFocusChanged":{"id":null}})
    )
    .unwrap();
    assert_eq!(
        calls.recv_timeout(Duration::from_secs(5)).unwrap(),
        helper.0.id()
    );
    writeln!(stream, "{opened}").unwrap();
    assert_eq!(calls.recv_timeout(Duration::from_secs(5)).unwrap(), 123);

    drop(service);
    let (_service, calls) = scheduler(address);
    // No new focus event: the timer must restore the selection on the new owner.
    assert_eq!(calls.recv_timeout(Duration::from_secs(15)).unwrap(), 123);

    stream.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(
        calls.recv_timeout(Duration::from_secs(5)).unwrap(),
        helper.0.id()
    );
    assert!(
        !helper.0.wait().unwrap().success(),
        "stream errors must restart the service"
    );
}
