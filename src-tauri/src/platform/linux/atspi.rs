//! On-demand selection snapshots, never document/context collection. AT-SPI
//! coordinates are deliberately not treated as global Wayland coordinates.

use super::portal::{self, error};
use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use zbus::zvariant::OwnedObjectPath;

const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const TEXT: &str = "org.a11y.atspi.Text";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
const MAX_NODES: usize = 512;
const MAX_CHARS: i32 = 100_000;
type Object = (String, OwnedObjectPath);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub object: Object,
    pub process: u32,
    pub start: i32,
    pub end: i32,
    pub text: String,
    pub editable: bool,
}

fn state(states: &[u32], index: usize) -> bool {
    states
        .get(index / 32)
        .is_some_and(|bits| bits & (1 << (index % 32)) != 0)
}

fn usable(states: &[u32], role: u32) -> bool {
    role != 40
        && !state(states, 6)
        && state(states, 8)
        && state(states, 24)
        && state(states, 25)
        && state(states, 30)
}

pub(super) fn snapshot() -> Result<Snapshot, String> {
    if gtk::is_initialized_main_thread() {
        return Err("Accessibility selection reads must run on a worker thread".into());
    }
    if super::session() != super::Session::Wayland {
        return Err("A native Wayland session is required".into());
    }
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            let session = portal::connect().await?;
            let bus = zbus::Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
                .await
                .map_err(error)?;
            // No property writes to enable accessibility or background listener.
            let address: String = bus.call("GetAddress", &()).await.map_err(error)?;
            let connection = zbus::connection::Builder::address(address.as_str())
                .map_err(error)?
                .method_timeout(Duration::from_millis(300))
                .build()
                .await
                .map_err(error)?;
            snapshot_on(&connection, std::process::id()).await
        })
        .await
        .map_err(|_| "Accessibility selection scan timed out".to_string())?
    })
}

async fn accessible<'a>(
    connection: &'a zbus::Connection,
    object: &'a Object,
) -> Result<zbus::Proxy<'a>, String> {
    // Pin identities to unique bus names, never a well-known name a different
    // process could acquire while a translation is in flight.
    if !object.0.starts_with(':') {
        return Err("Accessibility object has no unique owner".into());
    }
    zbus::Proxy::new(connection, object.0.as_str(), object.1.as_str(), ACCESSIBLE)
        .await
        .map_err(error)
}

async fn snapshot_on(connection: &zbus::Connection, own_process: u32) -> Result<Snapshot, String> {
    let registry = zbus::Proxy::new(connection, "org.a11y.atspi.Registry", ROOT, ACCESSIBLE)
        .await
        .map_err(error)?;
    let children: Vec<Object> = registry.call("GetChildren", &()).await.map_err(error)?;
    if children.len() > MAX_NODES {
        return Err("Accessibility tree exceeds the scan budget".into());
    }
    let mut queue: VecDeque<_> = children
        .into_iter()
        .take(MAX_NODES)
        .map(|object| (object, false))
        .collect();
    let mut visited = HashSet::new();
    let mut found = None;
    while let Some((object, ancestor_active)) = queue.pop_front() {
        if !visited.insert(object.clone()) {
            continue;
        }
        if visited.len() > MAX_NODES {
            return Err("Accessibility tree exceeds the scan budget".into());
        }
        let Ok(proxy) = accessible(connection, &object).await else {
            continue;
        };
        let Ok(states): Result<Vec<u32>, _> = proxy.call("GetState", &()).await else {
            continue;
        };
        let Ok(role): Result<u32, _> = proxy.call("GetRole", &()).await else {
            continue;
        };
        // Password/defunct subtrees are never inspected, even for reading.
        if role == 40 || state(&states, 6) {
            continue;
        }
        let active = ancestor_active || state(&states, 1);
        if active && state(&states, 12) && usable(&states, role) {
            let interfaces: Vec<String> = proxy.call("GetInterfaces", &()).await.map_err(error)?;
            if interfaces.iter().any(|interface| interface == TEXT) {
                let current = selected(connection, &object, &states, role, own_process).await?;
                if found.replace(current).is_some() {
                    return Err("Multiple focused accessibility selections; no safe target".into());
                }
            }
        }
        let Ok(children): Result<Vec<Object>, _> = proxy.call("GetChildren", &()).await else {
            continue;
        };
        if children.len() + queue.len() + visited.len() > MAX_NODES {
            return Err("Accessibility tree exceeds the scan budget".into());
        }
        queue.extend(children.into_iter().map(|child| (child, active)));
    }
    found.ok_or_else(|| "No focused accessible selection; source app may not support AT-SPI".into())
}

async fn selected(
    connection: &zbus::Connection,
    object: &Object,
    states: &[u32],
    role: u32,
    own_process: u32,
) -> Result<Snapshot, String> {
    let dbus = zbus::fdo::DBusProxy::new(connection).await.map_err(error)?;
    let process = dbus
        .get_connection_unix_process_id(object.0.as_str().try_into().map_err(error)?)
        .await
        .map_err(error)?;
    if process == own_process {
        return Err("Cannot capture this application's own input".into());
    }
    let proxy = zbus::Proxy::new(connection, object.0.as_str(), object.1.as_str(), TEXT)
        .await
        .map_err(error)?;
    let count: i32 = proxy.call("GetNSelections", &()).await.map_err(error)?;
    if count != 1 {
        return Err("Exactly one nonempty text selection is required".into());
    }
    let (start, end): (i32, i32) = proxy.call("GetSelection", &(0i32,)).await.map_err(error)?;
    if start < 0 || end <= start || end - start > MAX_CHARS {
        return Err("Accessibility selection range is invalid or too large".into());
    }
    // The only text read is this selected range. Never request GetText(0,-1).
    let text: String = proxy.call("GetText", &(start, end)).await.map_err(error)?;
    if text.chars().count() != (end - start) as usize || text.trim().is_empty() {
        return Err("Accessibility selection changed or returned an inconsistent range".into());
    }
    let confirm: (i32, i32) = proxy.call("GetSelection", &(0i32,)).await.map_err(error)?;
    let a11y = accessible(connection, object).await?;
    let states_after: Vec<u32> = a11y.call("GetState", &()).await.map_err(error)?;
    if confirm != (start, end) || !state(&states_after, 12) || !usable(&states_after, role) {
        return Err("Accessibility focus or selection changed during capture".into());
    }
    Ok(Snapshot {
        object: object.clone(),
        process,
        start,
        end,
        text,
        editable: role != 60
            && state(states, 7)
            && !state(states, 43)
            && state(&states_after, 7)
            && !state(&states_after, 43),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    #[test]
    #[ignore = "requires the isolated KWin/Qt editor from integrations/kwin/native/smoke.sh"]
    fn real_qt_selection() {
        let result = snapshot().expect("Qt editor did not expose a focused AT-SPI selection");
        assert_eq!(result.text, "original");
        assert_eq!((result.start, result.end), (7, 15));
        assert!(result.editable);
        eprintln!("Real AT-SPI/Qt: editable selection range verified without surrounding text");
    }

    struct Accessible {
        role: u32,
        states: Vec<u32>,
        children: Vec<Object>,
        lose_focus: bool,
        calls: AtomicU32,
    }
    #[zbus::interface(name = "org.a11y.atspi.Accessible")]
    impl Accessible {
        fn get_role(&self) -> u32 {
            self.role
        }
        fn get_state(&self) -> Vec<u32> {
            let mut states = self.states.clone();
            if self.lose_focus && self.calls.fetch_add(1, Ordering::Relaxed) > 0 {
                states[0] &= !(1 << 12);
            }
            states
        }
        fn get_children(&self) -> Vec<Object> {
            self.children.clone()
        }
        fn get_interfaces(&self) -> Vec<String> {
            vec![TEXT.into()]
        }
    }
    struct Text {
        count: i32,
        range: (i32, i32),
        reads: Arc<AtomicU32>,
        text: String,
    }
    #[zbus::interface(name = "org.a11y.atspi.Text")]
    impl Text {
        fn get_n_selections(&self) -> i32 {
            self.count
        }
        fn get_selection(&self, index: i32) -> (i32, i32) {
            assert_eq!(index, 0);
            self.range
        }
        fn get_text(&self, start: i32, end: i32) -> String {
            assert_eq!(
                (start, end),
                self.range,
                "Never request context or the full document"
            );
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.text.clone()
        }
    }
    fn editable_states() -> Vec<u32> {
        vec![
            (1 << 1) | (1 << 7) | (1 << 8) | (1 << 12) | (1 << 24) | (1 << 25) | (1 << 30),
            0,
        ]
    }
    async fn fixture(
        role: u32,
        states: Vec<u32>,
        range: (i32, i32),
        count: i32,
        text: &str,
        lose_focus: bool,
    ) -> (
        super::super::test_bus::Bus,
        zbus::Connection,
        zbus::Connection,
        Arc<AtomicU32>,
    ) {
        let bus = super::super::test_bus::Bus::start();
        let server = bus.connection().await;
        server
            .request_name("org.a11y.atspi.Registry")
            .await
            .unwrap();
        let path = OwnedObjectPath::try_from("/org/a11y/atspi/accessible/selection").unwrap();
        let object = (server.unique_name().unwrap().to_string(), path.clone());
        server
            .object_server()
            .at(
                ROOT,
                Accessible {
                    role: 14,
                    states: vec![0, 0],
                    children: vec![object],
                    lose_focus: false,
                    calls: AtomicU32::new(0),
                },
            )
            .await
            .unwrap();
        server
            .object_server()
            .at(
                path.clone(),
                Accessible {
                    role,
                    states,
                    children: vec![],
                    lose_focus,
                    calls: AtomicU32::new(0),
                },
            )
            .await
            .unwrap();
        let reads = Arc::new(AtomicU32::new(0));
        server
            .object_server()
            .at(
                path,
                Text {
                    count,
                    range,
                    reads: reads.clone(),
                    text: text.into(),
                },
            )
            .await
            .unwrap();
        let client = bus.connection().await;
        (bus, server, client, reads)
    }

    #[tokio::test]
    async fn reads_only_the_unicode_selected_range_and_pins_unique_owner() {
        let (_bus, server, client, reads) =
            fixture(42, editable_states(), (5, 8), 1, "你a🙂", false).await;
        let result = snapshot_on(&client, u32::MAX).await.unwrap();
        assert_eq!(result.text, "你a🙂");
        assert_eq!((result.start, result.end), (5, 8));
        assert!(result.editable);
        assert_eq!(result.object.0, server.unique_name().unwrap().as_str());
        assert_eq!(result.process, std::process::id());
        assert_eq!(reads.load(Ordering::Relaxed), 1);
        assert!(
            snapshot_on(&client, std::process::id())
                .await
                .unwrap_err()
                .contains("own input")
        );
    }

    #[tokio::test]
    async fn password_defunct_unfocused_and_invalid_ranges_never_read_text() {
        let mut defunct = editable_states();
        defunct[0] |= 1 << 6;
        let mut unfocused = editable_states();
        unfocused[0] &= !(1 << 12);
        for (role, states, range, count) in [
            (40, editable_states(), (0, 3), 1),
            (42, defunct, (0, 3), 1),
            (42, unfocused, (0, 3), 1),
            (42, editable_states(), (-1, 3), 1),
            (42, editable_states(), (0, 100_001), 1),
            (42, editable_states(), (0, 3), 2),
        ] {
            let (_bus, _server, client, reads) =
                fixture(role, states, range, count, "abc", false).await;
            assert!(snapshot_on(&client, u32::MAX).await.is_err());
            assert_eq!(reads.load(Ordering::Relaxed), 0);
        }
    }

    #[tokio::test]
    async fn rejects_focus_changes_and_inconsistent_text_and_marks_read_only() {
        let (_bus, _server, client, _) =
            fixture(42, editable_states(), (0, 3), 1, "abc", true).await;
        assert!(snapshot_on(&client, u32::MAX).await.is_err());
        let (_bus, _server, client, _) =
            fixture(42, editable_states(), (0, 3), 1, "ab", false).await;
        assert!(snapshot_on(&client, u32::MAX).await.is_err());
        let mut states = editable_states();
        states[1] |= 1 << (43 - 32);
        let (_bus, _server, client, _) = fixture(42, states, (0, 3), 1, "abc", false).await;
        assert!(!snapshot_on(&client, u32::MAX).await.unwrap().editable);
    }
    #[test]
    fn states_are_bitsets_not_an_array_of_enum_values() {
        let states = [
            (1 << 7) | (1 << 8) | (1 << 12) | (1 << 24) | (1 << 25) | (1 << 30),
            1 << (43 - 32),
        ];
        assert!(state(&states, 7) && state(&states, 43));
        assert!(!state(&states, 6) && !state(&[], 43));
        assert!(usable(&states, 42));
        assert!(!usable(&states, 40));
        assert!(!usable(&[states[0] | (1 << 6)], 42));
    }

    #[tokio::test]
    async fn ambiguous_focus_and_oversized_registry_are_rejected() {
        let (_bus, server, client, reads) =
            fixture(42, editable_states(), (0, 3), 1, "abc", false).await;
        let first = (
            server.unique_name().unwrap().to_string(),
            OwnedObjectPath::try_from("/org/a11y/atspi/accessible/selection").unwrap(),
        );
        let second = (
            first.0.clone(),
            OwnedObjectPath::try_from("/org/a11y/atspi/accessible/second").unwrap(),
        );
        server
            .object_server()
            .at(
                second.1.clone(),
                Accessible {
                    role: 42,
                    states: editable_states(),
                    children: vec![],
                    lose_focus: false,
                    calls: AtomicU32::new(0),
                },
            )
            .await
            .unwrap();
        server
            .object_server()
            .at(
                second.1.clone(),
                Text {
                    count: 1,
                    range: (0, 3),
                    reads: reads.clone(),
                    text: "abc".into(),
                },
            )
            .await
            .unwrap();
        server
            .object_server()
            .remove::<Accessible, _>(ROOT)
            .await
            .unwrap();
        server
            .object_server()
            .at(
                ROOT,
                Accessible {
                    role: 14,
                    states: vec![0, 0],
                    children: vec![first.clone(), second],
                    lose_focus: false,
                    calls: AtomicU32::new(0),
                },
            )
            .await
            .unwrap();
        assert!(
            snapshot_on(&client, u32::MAX)
                .await
                .unwrap_err()
                .contains("Multiple focused")
        );
        server
            .object_server()
            .remove::<Accessible, _>(ROOT)
            .await
            .unwrap();
        server
            .object_server()
            .at(
                ROOT,
                Accessible {
                    role: 14,
                    states: vec![0, 0],
                    children: vec![first; MAX_NODES + 1],
                    lose_focus: false,
                    calls: AtomicU32::new(0),
                },
            )
            .await
            .unwrap();
        reads.store(0, Ordering::Relaxed);
        assert!(
            snapshot_on(&client, u32::MAX)
                .await
                .unwrap_err()
                .contains("scan budget")
        );
        assert_eq!(reads.load(Ordering::Relaxed), 0);
    }
}
