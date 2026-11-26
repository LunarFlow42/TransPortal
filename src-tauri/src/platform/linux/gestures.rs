//! Optional KWin plugin. The only incoming signal contains a recognized
//! gesture, source PID, and focus/input epoch, never keyboard event data.
use crate::platform::gestures::{GestureEvent, GestureObserver, GestureSource, InkKey};
use futures_util::StreamExt;
use std::sync::{Mutex, OnceLock, mpsc::SyncSender};
use tokio::sync::{oneshot, watch};

const SERVICE: &str = "org.kde.KWin";
const PATH: &str = "/org/kde/KWin/TransPortalGestures";
const IFACE: &str = "org.kde.KWin.TransPortalGestures1";

#[derive(Default, Clone, Copy, PartialEq, Eq)]
struct Config {
    copy: bool,
    ink: i32,
    suspended: bool,
}
type Active = (u64, zbus::Proxy<'static>, bool, bool);
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn settings() -> &'static watch::Sender<Config> {
    static SETTINGS: OnceLock<watch::Sender<Config>> = OnceLock::new();
    SETTINGS.get_or_init(|| watch::Sender::new(Config::default()))
}

pub(super) fn active() -> bool {
    ACTIVE.lock().is_ok_and(|active| active.is_some())
}

pub(super) fn replacement_available() -> bool {
    ACTIVE.lock().is_ok_and(|active| {
        active
            .as_ref()
            .is_some_and(|(_, _, replacement, _)| *replacement)
    })
}
#[cfg(test)]
fn remote_input_available() -> bool {
    ACTIVE.lock().is_ok_and(|active| {
        active
            .as_ref()
            .is_some_and(|(_, _, _, available)| *available)
    })
}
pub(super) fn set_remote_input_authorized(authorized: bool) -> Result<(), String> {
    let proxy = plugin_proxy()?;
    let result: bool =
        tauri::async_runtime::block_on(async { proxy.call("SetRemoteInput", &(authorized)).await })
            .map_err(super::portal::error)?;
    if !result {
        return Err("KWin refused the RemoteDesktop input grant".into());
    }
    if let Ok(mut active) = ACTIVE.lock()
        && let Some((_, _, _, available)) = active.as_mut()
    {
        *available = authorized;
    }
    Ok(())
}

pub(super) fn remote_proxy() -> Result<zbus::Proxy<'static>, String> {
    ACTIVE
        .lock()
        .map_err(super::portal::error)?
        .as_ref()
        .filter(|(_, _, _, available)| *available)
        .map(|(_, proxy, _, _)| proxy.clone())
        .ok_or_else(|| "KWin remote input is not enabled for this executable".into())
}

pub(super) fn plugin_proxy() -> Result<zbus::Proxy<'static>, String> {
    ACTIVE
        .lock()
        .map_err(super::portal::error)?
        .as_ref()
        .map(|(_, proxy, _, _)| proxy.clone())
        .ok_or_else(|| "KWin TransPortal plugin is not connected".into())
}

#[derive(Default)]
pub struct KWinGestures;
struct Observer(Option<oneshot::Sender<()>>);
impl GestureObserver for Observer {}
impl Drop for Observer {
    fn drop(&mut self) {
        if let Some(stop) = self.0.take() {
            let _ = stop.send(());
        }
    }
}

impl GestureSource for KWinGestures {
    fn discovers_availability() -> bool {
        super::session() == super::Session::Wayland
    }
    fn set_double_copy(copy: bool) {
        settings().send_if_modified(|config| update(&mut config.copy, copy));
    }
    fn suspend(suspended: bool) {
        settings().send_if_modified(|config| update(&mut config.suspended, suspended));
    }
    fn set_ink_key(key: Option<InkKey>) {
        let ink = match key {
            None => 0,
            Some(InkKey::Backquote) => 1,
            Some(InkKey::CapsLock) => 2,
            Some(InkKey::Control) => 3,
            Some(InkKey::Alt) => 4,
            Some(InkKey::Shift) => 5,
            Some(InkKey::Function(n)) => 10 + i32::from(n),
        };
        settings().send_if_modified(|config| update(&mut config.ink, ink));
    }
    fn matches_focus(process: u32, epoch: u64) -> bool {
        let proxy = ACTIVE
            .lock()
            .ok()
            .and_then(|value| value.as_ref().map(|(_, proxy, _, _)| proxy.clone()));
        proxy.is_some_and(|proxy| {
            tauri::async_runtime::block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_millis(300),
                    proxy.call("MatchesFocus", &(process, epoch)),
                )
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(false)
            })
        })
    }
    fn observe(
        &self,
        events: SyncSender<GestureEvent>,
    ) -> Result<Box<dyn GestureObserver>, String> {
        if super::session() != super::Session::Wayland {
            return Err("KWin gestures require a Wayland session".into());
        }
        // Startup can run on the GTK thread; no blocking D-Bus handshake there.
        // Active status is only published after the plugin acknowledges Configure.
        let (stop, stopped) = oneshot::channel();
        tauri::async_runtime::spawn(observe(events, stopped));
        Ok(Box::new(Observer(Some(stop))))
    }
}

fn update<T: PartialEq>(target: &mut T, value: T) -> bool {
    if *target == value {
        return false;
    }
    *target = value;
    true
}

async fn configure(proxy: &zbus::Proxy<'_>, config: Config) -> Result<(), String> {
    let accepted: bool = proxy
        .call("Configure", &(config.copy, config.ink, config.suspended))
        .await
        .map_err(super::portal::error)?;
    if !accepted {
        return Err(
            "The KWin gesture plugin already has a listener or rejected configuration".into(),
        );
    }
    Ok(())
}

async fn observe(events: SyncSender<GestureEvent>, mut stopped: oneshot::Receiver<()>) {
    use std::time::Duration;
    let mut settings = settings().subscribe();
    // Discover optional plugin installation and removal without observing input
    // or retaining old callbacks while disconnected.
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            _ = attach(&events, &mut settings) => {},
        }
        tokio::select! {
            _ = &mut stopped => break,
            _ = tokio::time::sleep(Duration::from_secs(3)) => {},
        }
    }
}

struct Attachment<'a>(u64, &'a SyncSender<GestureEvent>);
impl Drop for Attachment<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock()
            && active.as_ref().is_some_and(|(id, _, _, _)| *id == self.0)
        {
            active.take();
        }
        let _ = self.1.try_send(GestureEvent::AvailabilityChanged);
    }
}

async fn attach(
    events: &SyncSender<GestureEvent>,
    settings: &mut watch::Receiver<Config>,
) -> Result<(), String> {
    let connection = super::portal::connect().await?;
    attach_on(connection, events, settings).await
}

async fn attach_on(
    connection: zbus::Connection,
    events: &SyncSender<GestureEvent>,
    settings: &mut watch::Receiver<Config>,
) -> Result<(), String> {
    let dbus = zbus::fdo::DBusProxy::new(&connection)
        .await
        .map_err(super::portal::error)?;
    let owner = dbus
        .get_name_owner(SERVICE.try_into().map_err(super::portal::error)?)
        .await
        .map_err(super::portal::error)?;
    let proxy = zbus::Proxy::new(&connection, owner, PATH, IFACE)
        .await
        .map_err(super::portal::error)?;
    let mut signals = proxy
        .receive_signal("Triggered")
        .await
        .map_err(super::portal::error)?;
    let mut current = *settings.borrow_and_update();
    configure(&proxy, current).await?;
    struct ConnectionGuard(zbus::Connection);
    impl Drop for ConnectionGuard {
        fn drop(&mut self) {
            let connection = self.0.clone();
            // ACTIVE can hold a proxy during cancellation. Explicit closure
            // ensures KWin sees owner loss and uninstalls the native spy.
            tauri::async_runtime::spawn(async move {
                let _ = connection.close().await;
            });
        }
    }
    let _connection = ConnectionGuard(connection.clone());
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let replacement: bool = proxy
        .call("ReplacementAvailable", &())
        .await
        .unwrap_or(false);
    let remote_input: bool = proxy.call("InputAvailable", &()).await.unwrap_or(false);
    *ACTIVE.lock().map_err(super::portal::error)? =
        Some((id, proxy.clone(), replacement, remote_input));
    let _attachment = Attachment(id, events);
    let _ = events.try_send(GestureEvent::AvailabilityChanged);
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(3));
    heartbeat.tick().await;
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                configure(&proxy, current).await?;
                let replacement: bool = proxy.call("ReplacementAvailable", &()).await.unwrap_or(false);
                if let Ok(mut active) = ACTIVE.lock()
                    && let Some((current_id, _, available, _)) = active.as_mut()
                    && *current_id == id && update(available, replacement) {
                    let _ = events.try_send(GestureEvent::AvailabilityChanged);
                }
                let remote_input: bool = proxy.call("InputAvailable", &()).await.unwrap_or(false);
                if let Ok(mut active) = ACTIVE.lock()
                    && let Some((current_id, _, _, available)) = active.as_mut()
                    && *current_id == id && update(available, remote_input) {
                    let _ = events.try_send(GestureEvent::AvailabilityChanged);
                }
                if remote_input && !super::remote_desktop::active() {
                    let revoked: bool = proxy.call("SetRemoteInput", &(false)).await.unwrap_or(false);
                    if revoked {
                        if let Ok(mut active) = ACTIVE.lock()
                            && let Some((current_id, _, _, available)) = active.as_mut()
                            && *current_id == id { *available = false; }
                        let _ = events.try_send(GestureEvent::AvailabilityChanged);
                    }
                }
            },
            result = settings.changed() => {
                result.map_err(super::portal::error)?;
                current = *settings.borrow_and_update();
                configure(&proxy, current).await?;
            }
            signal = signals.next() => {
                let signal = signal.ok_or("KWin disconnected")?;
                if let Ok((kind, process, epoch)) = signal.body().deserialize::<(String, u32, u64)>()
                    && let Some(event) = decode(&kind, process, epoch, *settings.borrow()) {
                    let _ = events.try_send(event);
                }
            }
        }
    }
}

fn decode(kind: &str, process: u32, epoch: u64, config: Config) -> Option<GestureEvent> {
    if config.suspended || process == 0 || process == std::process::id() || epoch == 0 {
        return None;
    }
    let double_copy = match kind {
        "copy" if config.copy => true,
        "ink" if config.ink != 0 => false,
        _ => return None,
    };
    Some(GestureEvent::CompositorSelection {
        double_copy,
        process,
        epoch,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    };

    struct FakePlugin {
        calls: Arc<AtomicU32>,
        remote: Arc<AtomicBool>,
    }
    #[zbus::interface(name = "org.kde.KWin.TransPortalGestures1")]
    impl FakePlugin {
        fn configure(&self, copy: bool, ink: i32, suspended: bool) -> bool {
            assert!(copy && ink == 5 && !suspended);
            self.calls.fetch_add(1, Ordering::Relaxed);
            true
        }
        fn replacement_available(&self) -> bool {
            false
        }
        fn matches_focus(&self, process: u32, epoch: u64) -> bool {
            process == u32::MAX && epoch == 12
        }
        fn set_remote_input(&self, enabled: bool) -> bool {
            self.remote.store(enabled, Ordering::Release);
            true
        }
        fn input_available(&self) -> bool {
            self.remote.load(Ordering::Acquire)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plugin_attachment_filters_signals_and_clears_availability_on_disconnect() {
        let bus = super::super::test_bus::Bus::start();
        let server = bus.connection().await;
        server.request_name(SERVICE).await.unwrap();
        let calls = Arc::new(AtomicU32::new(0));
        let remote = Arc::new(AtomicBool::new(false));
        server
            .object_server()
            .at(
                PATH,
                FakePlugin {
                    calls: calls.clone(),
                    remote: remote.clone(),
                },
            )
            .await
            .unwrap();
        let client = bus.connection().await;
        let settings = watch::Sender::new(Config {
            copy: true,
            ink: 5,
            suspended: false,
        });
        let mut config = settings.subscribe();
        let (events, received) = std::sync::mpsc::sync_channel(16);
        let task = tokio::spawn(async move { attach_on(client, &events, &mut config).await });
        let event = tokio::task::spawn_blocking(move || {
            (
                received
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap(),
                received,
            )
        })
        .await
        .unwrap();
        assert!(matches!(event.0, GestureEvent::AvailabilityChanged));
        let received = event.1;
        assert!(active());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert!(!remote_input_available());
        tokio::task::block_in_place(|| set_remote_input_authorized(true)).unwrap();
        assert!(remote_input_available());
        tokio::task::block_in_place(|| set_remote_input_authorized(false)).unwrap();
        assert!(!remote_input_available());
        for kind in ["raw_key", "copy"] {
            server
                .emit_signal(
                    None::<&str>,
                    PATH,
                    IFACE,
                    "Triggered",
                    &(kind, u32::MAX, 12u64),
                )
                .await
                .unwrap();
        }
        let (event, received) = tokio::task::spawn_blocking(move || {
            (
                received
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap(),
                received,
            )
        })
        .await
        .unwrap();
        assert!(matches!(
            event,
            GestureEvent::CompositorSelection {
                double_copy: true,
                process: u32::MAX,
                epoch: 12
            }
        ));
        assert!(
            received.try_recv().is_err(),
            "Raw key data must never enter the core"
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(!active());
        assert!(matches!(
            received
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            GestureEvent::AvailabilityChanged
        ));
    }

    #[tokio::test]
    #[ignore = "requires the isolated KWin fixture built with TRANSPORTAL_KWIN_SMOKE_TESTS"]
    async fn real_kwin_copy_and_ink_events_reach_rust_and_read_the_selected_range() {
        use crate::platform::selection::SelectionOps;

        KWinGestures::set_double_copy(true);
        KWinGestures::set_ink_key(Some(InkKey::Shift));
        let (events, receiver) = std::sync::mpsc::sync_channel(8);
        let _observer = KWinGestures.observe(events).unwrap();
        let started = std::time::Instant::now();
        while !active() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "KWin plugin did not attach"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(matches!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            GestureEvent::AvailabilityChanged
        ));
        let proxy = plugin_proxy().unwrap();
        let key = |key: i32, scan: u32, state: i32, ctrl: bool, other: bool| {
            let proxy = proxy.clone();
            async move {
                let accepted: bool = proxy
                    .call("TestKeyEvent", &(key, scan, state, ctrl, other))
                    .await
                    .unwrap();
                assert!(
                    accepted,
                    "test-only synthetic event rejected by KWin plugin"
                );
            }
        };
        let (control, copy) = (0x01000021, 0x43); // Qt::Key_Control / Qt::Key_C
        key(control, 29, 0, false, true).await;
        key(copy, 46, 0, true, true).await;
        key(copy, 46, 1, true, true).await;
        key(control, 29, 1, false, true).await;
        key(control, 29, 0, false, true).await;
        key(copy, 46, 0, true, true).await;
        key(copy, 46, 1, true, true).await;
        let (event, receiver) = tokio::task::spawn_blocking(move || {
            (
                receiver
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap(),
                receiver,
            )
        })
        .await
        .unwrap();
        let (double_copy, process, epoch) = match event {
            GestureEvent::CompositorSelection {
                double_copy,
                process,
                epoch,
            } => (double_copy, process, epoch),
            other => panic!("unexpected KWin gesture: {other:?}"),
        };
        assert!(double_copy);
        tokio::task::spawn_blocking(move || {
            assert!(KWinGestures::matches_focus(process, epoch));
            let outcome = super::super::selection::LinuxSelection::default()
                .read_compositor_selection(process)
                .unwrap();
            assert_eq!(outcome.text, "original");
            assert!(KWinGestures::matches_focus(process, epoch));
        })
        .await
        .unwrap();

        key(control, 29, 1, false, true).await;
        key(0x01000020, 42, 0, false, true).await; // Qt::Key_Shift
        key(0x01000020, 42, 1, false, true).await;
        let (event, receiver) = tokio::task::spawn_blocking(move || {
            (
                receiver
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap(),
                receiver,
            )
        })
        .await
        .unwrap();
        let (double_copy, process, epoch) = match event {
            GestureEvent::CompositorSelection {
                double_copy,
                process,
                epoch,
            } => (double_copy, process, epoch),
            other => panic!("unexpected KWin gesture: {other:?}"),
        };
        assert!(!double_copy);
        tokio::task::spawn_blocking(move || {
            assert!(KWinGestures::matches_focus(process, epoch));
            let outcome = super::super::selection::LinuxSelection::default()
                .read_compositor_selection(process)
                .unwrap();
            assert_eq!(outcome.text, "original");
            assert!(KWinGestures::matches_focus(process, epoch));
        })
        .await
        .unwrap();
        key(0x41, 30, 0, false, true).await;
        tokio::task::spawn_blocking(move || {
            assert!(!KWinGestures::matches_focus(process, epoch));
        })
        .await
        .unwrap();
        key(control, 29, 0, false, true).await;
        key(copy, 46, 0, true, true).await;
        key(copy, 46, 2, true, true).await;
        key(copy, 46, 1, true, true).await;
        key(control, 29, 1, false, true).await;
        key(0x01000020, 42, 0, false, true).await;
        key(0x41, 30, 0, false, true).await;
        key(0x01000020, 42, 1, false, true).await;
        let configured: bool = proxy
            .call("TestConfigure", &(false, 5, false))
            .await
            .unwrap();
        assert!(configured);
        for _ in 0..2 {
            key(copy, 46, 0, true, true).await;
            key(copy, 46, 1, true, true).await;
        }
        let configured: bool = proxy.call("TestConfigure", &(true, 5, true)).await.unwrap();
        assert!(configured);
        let accepted: bool = proxy
            .call("TestKeyEvent", &(0x01000020i32, 42u32, 0i32, false, true))
            .await
            .unwrap();
        assert!(!accepted, "suspended observer must not accept test input");
        tokio::task::spawn_blocking(move || {
            assert!(
                receiver
                    .recv_timeout(std::time::Duration::from_millis(150))
                    .is_err(),
                "cancelled or disabled sequence emitted a selection event"
            );
        })
        .await
        .unwrap();
        eprintln!(
            "KWin gesture negatives: stale epoch, repeat, chord, disabled copy and suspended input rejected"
        );
        eprintln!(
            "Real KWin Rust integration (test-only synthetic events): double-copy + ink -> D-Bus -> Rust focus check -> actual AT-SPI selection succeeded; no physical key claim"
        );
    }
    #[test]
    fn only_enabled_recognized_gestures_are_forwarded() {
        let config = Config {
            copy: true,
            ink: 0,
            suspended: false,
        };
        assert!(matches!(
            decode("copy", u32::MAX, 8, config),
            Some(GestureEvent::CompositorSelection {
                double_copy: true,
                epoch: 8,
                ..
            })
        ));
        for kind in ["ink", "key", "Ctrl+C", ""] {
            assert!(decode(kind, u32::MAX, 8, config).is_none());
        }
        assert!(decode("copy", std::process::id(), 8, config).is_none());
        assert!(
            decode(
                "copy",
                u32::MAX,
                8,
                Config {
                    suspended: true,
                    ..config
                }
            )
            .is_none()
        );
    }
}
