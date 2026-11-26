//! Explicit, keyboard-only authorization. No screenshot, pointer control,
//! persistence tokens, key observation or arbitrary-input IPC is requested.
//! The first transport is Portal D-Bus; EIS is NOT connected, so future Notify
//! calls must never share a session with a libei sender.

use super::portal::{self, error};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::time::Duration;
use zbus::zvariant::{OwnedObjectPath, Value};

const INTERFACE: &str = "org.freedesktop.portal.RemoteDesktop";
static ACTIVE_SESSIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(super) fn active() -> bool {
    ACTIVE_SESSIONS.load(std::sync::atomic::Ordering::Acquire) > 0
}

pub(super) struct KeyboardSession {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    connection: zbus::Connection,
    path: OwnedObjectPath,
    active: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl KeyboardSession {
    pub fn authorize(cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Self, String> {
        if super::session() != super::Session::Wayland {
            return Err("RemoteDesktop authorization requires Wayland".into());
        }
        if gtk::is_initialized_main_thread() {
            return Err("Input authorization must run on a worker thread".into());
        }
        tauri::async_runtime::block_on(async {
            Self::authorize_on(portal::connect().await?, cancelled).await
        })
    }

    async fn authorize_on(
        connection: zbus::Connection,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Self, String> {
        if cancelled() {
            return Err("Input authorization cancelled".into());
        }
        let proxy = portal::proxy(&connection, INTERFACE).await?;
        let version: u32 = proxy.get_property("version").await.map_err(error)?;
        let devices: u32 = proxy
            .get_property("AvailableDeviceTypes")
            .await
            .map_err(error)?;
        if version == 0 || devices & 1 == 0 {
            return Err("RemoteDesktop portal does not provide keyboard input".into());
        }
        let create = portal::token();
        let session_token = portal::token();
        let options = HashMap::from([
            ("handle_token", Value::from(create.as_str())),
            ("session_handle_token", Value::from(session_token.as_str())),
        ]);
        let values = portal::request(
            &connection,
            &proxy,
            "CreateSession",
            &(options,),
            &create,
            cancelled,
        )
        .await?;
        let path = values
            .get("session_handle")
            .and_then(|value| <&str>::try_from(value).ok())
            .ok_or("RemoteDesktop returned no session handle")?;
        let sender = connection
            .unique_name()
            .ok_or("Missing bus name")?
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        if path != format!("{}/session/{sender}/{session_token}", portal::DESKTOP) {
            return Err("Unexpected RemoteDesktop session handle".into());
        }
        let mut guard = SessionGuard {
            connection: connection.clone(),
            path: OwnedObjectPath::try_from(path.to_owned()).map_err(error)?,
            closed: false,
        };
        let session = zbus::Proxy::new(
            &connection,
            portal::DESTINATION,
            guard.path.clone(),
            "org.freedesktop.portal.Session",
        )
        .await
        .map_err(error)?;
        let mut closed = session.receive_signal("Closed").await.map_err(error)?;
        let setup = async {
            let select = portal::token();
            portal::request(
                &connection,
                &proxy,
                "SelectDevices",
                &(
                    &guard.path,
                    HashMap::from([
                        ("handle_token", Value::from(select.as_str())),
                        ("types", Value::from(1u32)),
                    ]),
                ),
                &select,
                cancelled,
            )
            .await?;
            let start = portal::token();
            let values = portal::request(
                &connection,
                &proxy,
                "Start",
                &(
                    &guard.path,
                    "",
                    HashMap::from([("handle_token", Value::from(start.as_str()))]),
                ),
                &start,
                cancelled,
            )
            .await?;
            if values
                .get("devices")
                .and_then(|value| u32::try_from(value).ok())
                != Some(1)
            {
                return Err("Keyboard-only access was not granted".into());
            }
            Ok::<(), String>(())
        };
        let result = tokio::select! {
            result = setup => result,
            _ = closed.next() => Err("RemoteDesktop session closed during authorization".into()),
        };
        if let Err(error) = result {
            guard.close().await;
            return Err(error);
        }
        if cancelled() {
            guard.close().await;
            return Err("Input authorization cancelled".into());
        }
        let session_path = guard.path.clone();
        let active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let task_active = active.clone();
        ACTIVE_SESSIONS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let (stop, stopped) = tokio::sync::oneshot::channel();
        tauri::async_runtime::spawn(async move {
            tokio::select! {
                _ = stopped => {},
                _ = closed.next() => {},
            }
            if task_active.swap(false, std::sync::atomic::Ordering::AcqRel) {
                ACTIVE_SESSIONS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            }
            guard.close().await;
        });
        Ok(Self {
            stop: Some(stop),
            connection,
            path: session_path,
            active,
        })
    }

    pub fn active(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Send the fixed Ctrl+V sequence through the already-authorized
    /// keyboard-only RemoteDesktop session. This is a
    /// transport primitive, not a global listener and not used by the
    /// clipboard-free text-input replacement path.
    pub async fn paste_shortcut(
        &self,
        guard: &zbus::Proxy<'_>,
        process: u32,
    ) -> Result<(), String> {
        if !self.active() {
            return Err("RemoteDesktop keyboard permission is no longer active".into());
        }
        if process == 0 {
            return Err("A nonzero focused target PID is required".into());
        }
        let connection = self.connection.clone();
        let path = self.path.clone();
        let active = self.active.clone();
        async move {
            let session_path = path.clone();
            let epoch: u64 = guard.call("ArmInput", &(process,)).await.map_err(error)?;
            if epoch == 0 {
                return Err("KWin rejected the focused input target".into());
            }
            let proxy =
                zbus::Proxy::new(&connection, portal::DESTINATION, portal::DESKTOP, INTERFACE)
                    .await
                    .map_err(error)?;
            async fn send(
                proxy: &zbus::Proxy<'_>,
                session: &OwnedObjectPath,
                keysym: i32,
                pressed: bool,
            ) -> Result<(), String> {
                proxy
                    .call::<_, _, ()>(
                        "NotifyKeyboardKeysym",
                        &(
                            session,
                            HashMap::<String, zbus::zvariant::OwnedValue>::new(),
                            keysym,
                            u32::from(pressed),
                        ),
                    )
                    .await
                    .map_err(error)
            }
            async fn valid(
                guard: &zbus::Proxy<'_>,
                process: u32,
                epoch: u64,
            ) -> Result<(), String> {
                let ok: bool = guard
                    .call("ValidateInput", &(process, epoch))
                    .await
                    .map_err(error)?;
                if ok {
                    Ok(())
                } else {
                    Err("KWin focus changed; input was cancelled".into())
                }
            }
            const CONTROL_L: i32 = 0xffe3;
            const LOWER_V: i32 = 0x76;
            if !active.load(std::sync::atomic::Ordering::Acquire) {
                return Err("RemoteDesktop keyboard permission was revoked".into());
            }
            if let Err(error) = valid(guard, process, epoch).await {
                let _: Result<(), _> = guard.call("CancelInput", &(epoch,)).await;
                return Err(error);
            }
            if let Err(error) = send(&proxy, &session_path, CONTROL_L, true).await {
                let _: Result<(), _> = send(&proxy, &session_path, CONTROL_L, false).await;
                let _: Result<(), _> = guard.call("CancelInput", &(epoch,)).await;
                return Err(error);
            }
            let press_v = if valid(guard, process, epoch).await.is_ok() {
                send(&proxy, &session_path, LOWER_V, true).await
            } else {
                Err("KWin focus changed while preparing paste".into())
            };
            // Always attempt both releases after Control was pressed, even when
            // a portal call fails partway through; do not leave a modifier held.
            let release_v = send(&proxy, &session_path, LOWER_V, false).await;
            let release_control = send(&proxy, &session_path, CONTROL_L, false).await;
            let result = press_v.and(release_v).and(release_control);
            let _: Result<(), _> = guard.call("CancelInput", &(epoch,)).await;
            result
        }
        .await
    }
}

impl Drop for KeyboardSession {
    fn drop(&mut self) {
        if self.active.swap(false, std::sync::atomic::Ordering::AcqRel) {
            ACTIVE_SESSIONS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        }
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

struct SessionGuard {
    connection: zbus::Connection,
    path: OwnedObjectPath,
    closed: bool,
}
impl SessionGuard {
    async fn close(&mut self) {
        self.closed = true;
        close(&self.connection, &self.path).await;
    }
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        if !self.closed {
            let connection = self.connection.clone();
            let path = self.path.clone();
            tauri::async_runtime::spawn(async move {
                close(&connection, &path).await;
            });
        }
    }
}
async fn close(connection: &zbus::Connection, path: &OwnedObjectPath) {
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        if let Ok(proxy) = zbus::Proxy::new(
            connection,
            portal::DESTINATION,
            path.clone(),
            "org.freedesktop.portal.Session",
        )
        .await
        {
            let _: Result<(), _> = proxy.call("Close", &()).await;
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::super::portal::Values;
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    };

    struct FakeSession(Arc<AtomicU32>);
    #[zbus::interface(name = "org.freedesktop.portal.Session")]
    impl FakeSession {
        fn close(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    struct FakeRequest(Arc<AtomicU32>);
    #[zbus::interface(name = "org.freedesktop.portal.Request")]
    impl FakeRequest {
        fn close(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    struct FakeRemote {
        grant: u32,
        response: u32,
        closed: Arc<AtomicU32>,
        requests_closed: Arc<AtomicU32>,
        started: Arc<AtomicBool>,
        calls: Arc<Mutex<Vec<String>>>,
        keys: Arc<Mutex<Vec<(i32, u32)>>>,
        fail_v: Arc<AtomicBool>,
    }
    #[zbus::interface(name = "org.freedesktop.portal.RemoteDesktop")]
    impl FakeRemote {
        #[zbus(property, name = "version")]
        fn version(&self) -> u32 {
            2
        }
        #[zbus(property)]
        fn available_device_types(&self) -> u32 {
            7
        }
        async fn create_session(
            &self,
            options: Values,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            self.calls.lock().unwrap().push("create".into());
            let sender = sender(&header);
            let token = <&str>::try_from(options.get("session_handle_token").unwrap()).unwrap();
            let path = format!("{}/session/{sender}/{token}", portal::DESKTOP);
            connection
                .object_server()
                .at(path.clone(), FakeSession(self.closed.clone()))
                .await?;
            self.respond(
                connection,
                &sender,
                &options,
                0,
                HashMap::from([("session_handle", Value::from(path))]),
            )
            .await
        }
        async fn select_devices(
            &self,
            _session: OwnedObjectPath,
            options: Values,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            self.calls.lock().unwrap().push("select".into());
            assert_eq!(
                options
                    .get("types")
                    .and_then(|value| u32::try_from(value).ok()),
                Some(1)
            );
            assert!(
                !options.contains_key("restore_token") && !options.contains_key("persist_mode")
            );
            self.respond(connection, &sender(&header), &options, 0, HashMap::new())
                .await
        }
        async fn start(
            &self,
            _session: OwnedObjectPath,
            _parent: &str,
            options: Values,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            self.calls.lock().unwrap().push("start".into());
            self.started.store(true, Ordering::Release);
            self.respond(
                connection,
                &sender(&header),
                &options,
                self.response,
                HashMap::from([("devices", Value::from(self.grant))]),
            )
            .await
        }

        async fn notify_keyboard_keysym(
            &self,
            _session: OwnedObjectPath,
            options: Values,
            keysym: i32,
            state: u32,
        ) -> zbus::fdo::Result<()> {
            assert!(options.is_empty());
            self.keys.lock().unwrap().push((keysym, state));
            if self.fail_v.load(Ordering::Acquire) && (keysym, state) == (0x76, 1) {
                return Err(zbus::fdo::Error::Failed("Injected press failure".into()));
            }
            Ok(())
        }
    }
    fn sender(header: &zbus::message::Header<'_>) -> String {
        header
            .sender()
            .unwrap()
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_")
    }
    impl FakeRemote {
        async fn respond(
            &self,
            connection: &zbus::Connection,
            sender: &str,
            options: &Values,
            code: u32,
            values: HashMap<&str, Value<'_>>,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            let token = <&str>::try_from(options.get("handle_token").unwrap()).unwrap();
            let path =
                OwnedObjectPath::try_from(format!("{}/request/{sender}/{token}", portal::DESKTOP))
                    .unwrap();
            connection
                .object_server()
                .at(path.clone(), FakeRequest(self.requests_closed.clone()))
                .await?;
            if code != 99 {
                connection
                    .emit_signal(
                        None::<&str>,
                        path.clone(),
                        "org.freedesktop.portal.Request",
                        "Response",
                        &(code, values),
                    )
                    .await?;
            }
            Ok(path)
        }
    }
    struct Fixture {
        bus: super::super::test_bus::Bus,
        _server: zbus::Connection,
        closed: Arc<AtomicU32>,
        requests_closed: Arc<AtomicU32>,
        started: Arc<AtomicBool>,
        calls: Arc<Mutex<Vec<String>>>,
        keys: Arc<Mutex<Vec<(i32, u32)>>>,
        fail_v: Arc<AtomicBool>,
    }
    impl Fixture {
        async fn new(grant: u32, response: u32) -> Self {
            let bus = super::super::test_bus::Bus::start();
            let server = bus.connection().await;
            server.request_name(portal::DESTINATION).await.unwrap();
            let closed = Arc::new(AtomicU32::new(0));
            let requests_closed = Arc::new(AtomicU32::new(0));
            let started = Arc::new(AtomicBool::new(false));
            let calls = Arc::new(Mutex::new(Vec::new()));
            let keys = Arc::new(Mutex::new(Vec::new()));
            let fail_v = Arc::new(AtomicBool::new(false));
            server
                .object_server()
                .at(
                    portal::DESKTOP,
                    FakeRemote {
                        grant,
                        response,
                        closed: closed.clone(),
                        requests_closed: requests_closed.clone(),
                        started: started.clone(),
                        calls: calls.clone(),
                        keys: keys.clone(),
                        fail_v: fail_v.clone(),
                    },
                )
                .await
                .unwrap();
            Self {
                bus,
                _server: server,
                closed,
                requests_closed,
                started,
                calls,
                keys,
                fail_v,
            }
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn keyboard_authorization_target_checks_transport_and_failed_press_cleanup() {
        let fixture = Fixture::new(1, 0).await;
        let session = KeyboardSession::authorize_on(fixture.bus.connection().await, &|| false)
            .await
            .unwrap();
        assert_eq!(
            *fixture.calls.lock().unwrap(),
            ["create", "select", "start"]
        );
        assert_eq!(fixture.closed.load(Ordering::Acquire), 0);
        let guard_connection = fixture.bus.connection().await;
        let checks = Arc::new(AtomicU32::new(0));
        let limit = Arc::new(AtomicU32::new(u32::MAX));
        let cancelled = Arc::new(AtomicU32::new(0));
        fixture
            ._server
            .object_server()
            .at(
                "/guard",
                FakeGuard {
                    checks: checks.clone(),
                    limit: limit.clone(),
                    cancelled: cancelled.clone(),
                },
            )
            .await
            .unwrap();
        let guard = zbus::Proxy::new(
            &guard_connection,
            portal::DESTINATION,
            "/guard",
            "org.kde.KWin.TransPortalGestures1",
        )
        .await
        .unwrap();
        assert!(session.paste_shortcut(&guard, 0).await.is_err());
        assert!(session.paste_shortcut(&guard, 123).await.is_err());
        assert!(fixture.keys.lock().unwrap().is_empty());
        limit.store(0, Ordering::Release);
        assert!(session.paste_shortcut(&guard, u32::MAX).await.is_err());
        assert!(fixture.keys.lock().unwrap().is_empty());
        checks.store(0, Ordering::Release);
        limit.store(u32::MAX, Ordering::Release);
        session.paste_shortcut(&guard, u32::MAX).await.unwrap();
        assert_eq!(
            *fixture.keys.lock().unwrap(),
            [(0xffe3, 1), (0x76, 1), (0x76, 0), (0xffe3, 0)]
        );
        fixture.keys.lock().unwrap().clear();
        fixture.fail_v.store(true, Ordering::Release);
        assert!(session.paste_shortcut(&guard, u32::MAX).await.is_err());
        assert_eq!(
            *fixture.keys.lock().unwrap(),
            [(0xffe3, 1), (0x76, 1), (0x76, 0), (0xffe3, 0)]
        );
        fixture.keys.lock().unwrap().clear();
        checks.store(0, Ordering::Release);
        limit.store(1, Ordering::Release);
        assert!(session.paste_shortcut(&guard, u32::MAX).await.is_err());
        assert_eq!(
            *fixture.keys.lock().unwrap(),
            [(0xffe3, 1), (0x76, 0), (0xffe3, 0)]
        );
        assert_eq!(cancelled.load(Ordering::Acquire), 4);
        session.active.store(false, Ordering::Release);
        fixture.keys.lock().unwrap().clear();
        assert!(session.paste_shortcut(&guard, u32::MAX).await.is_err());
        assert!(fixture.keys.lock().unwrap().is_empty());
        drop(session);
        tokio::time::timeout(Duration::from_secs(2), async {
            while fixture.closed.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(fixture.closed.load(Ordering::Acquire), 1);
    }

    struct FakeGuard {
        checks: Arc<AtomicU32>,
        limit: Arc<AtomicU32>,
        cancelled: Arc<AtomicU32>,
    }
    #[zbus::interface(name = "org.kde.KWin.TransPortalGestures1")]
    impl FakeGuard {
        fn arm_input(&self, process: u32) -> u64 {
            if process == u32::MAX { 12 } else { 0 }
        }
        fn validate_input(&self, process: u32, epoch: u64) -> bool {
            process == u32::MAX
                && epoch == 12
                && self.checks.fetch_add(1, Ordering::AcqRel) < self.limit.load(Ordering::Acquire)
        }
        fn cancel_input(&self, epoch: u64) {
            assert_eq!(epoch, 12);
            self.cancelled.fetch_add(1, Ordering::AcqRel);
        }
    }
    #[tokio::test]
    async fn denial_and_wrong_device_grants_close_sessions() {
        for (grant, response) in [(0, 0), (2, 0), (3, 0), (1, 1), (1, 2)] {
            let fixture = Fixture::new(grant, response).await;
            assert!(
                KeyboardSession::authorize_on(fixture.bus.connection().await, &|| false)
                    .await
                    .is_err()
            );
            assert_eq!(fixture.closed.load(Ordering::Acquire), 1);
        }
    }
    #[tokio::test]
    async fn revoke_during_permission_dialog_closes_request_and_session() {
        let fixture = Fixture::new(1, 99).await;
        assert!(
            KeyboardSession::authorize_on(fixture.bus.connection().await, &|| fixture
                .started
                .load(Ordering::Acquire))
            .await
            .is_err()
        );
        assert_eq!(fixture.closed.load(Ordering::Acquire), 1);
        assert_eq!(fixture.requests_closed.load(Ordering::Acquire), 1);
    }
}
