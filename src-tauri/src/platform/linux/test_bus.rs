//! Private D-Bus daemon for portal tests. Never modifies DBUS_SESSION_BUS_ADDRESS
//! or connects to/requests permissions from the developer's desktop portal.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

pub(super) struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    pub fn start() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is required for Linux portal tests");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Self {
            child,
            address: address.trim().to_string(),
        }
    }
    pub async fn connection(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .unwrap()
            .method_timeout(std::time::Duration::from_secs(3))
            .build()
            .await
            .unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
