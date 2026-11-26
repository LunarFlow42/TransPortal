use super::portal::{self, Values, error};
use crate::platform::shortcuts::{
    ShortcutBinding, ShortcutEvent, ShortcutHandler, ShortcutOps, ShortcutRegistration,
};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use zbus::zvariant::{OwnedObjectPath, Value};

#[derive(Default)]
pub struct WaylandShortcuts {
    generation: AtomicU64,
    suspended: Arc<AtomicBool>,
    session: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// XDG shortcut syntax uses CTRL/ALT/SHIFT/LOGO, not Tauri's Super spelling.
fn preferred(value: &str) -> Result<String, String> {
    let parts: Vec<_> = value.split('+').map(str::trim).collect();
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err("Invalid portal shortcut".into());
    }
    let mut result = Vec::new();
    for part in &parts[..parts.len() - 1] {
        result.push(
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => "CTRL",
                "alt" => "ALT",
                "shift" => "SHIFT",
                "super" | "win" | "meta" => "LOGO",
                _ => return Err("Unsupported portal shortcut modifier".into()),
            }
            .to_string(),
        );
    }
    let key = parts.last().unwrap();
    let key = key
        .strip_prefix("Key")
        .or_else(|| key.strip_prefix("Digit"))
        .unwrap_or(key);
    let key = match key {
        "Space" => "space".into(),
        "Enter" => "Return".into(),
        "ArrowUp" => "Up".into(),
        "ArrowDown" => "Down".into(),
        "ArrowLeft" => "Left".into(),
        "ArrowRight" => "Right".into(),
        "Backspace" => "BackSpace".into(),
        "PageUp" => "Page_Up".into(),
        "PageDown" => "Page_Down".into(),
        "Backquote" => "grave".into(),
        "Minus" => "minus".into(),
        "Equal" => "equal".into(),
        "BracketLeft" => "bracketleft".into(),
        "BracketRight" => "bracketright".into(),
        "Backslash" => "backslash".into(),
        "Semicolon" => "semicolon".into(),
        "Quote" => "apostrophe".into(),
        "Comma" => "comma".into(),
        "Period" => "period".into(),
        "Slash" => "slash".into(),
        other if other.len() == 1 && other.is_ascii() => other.to_ascii_lowercase(),
        other => other.to_string(),
    };
    if !key
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err("Unsupported XKB shortcut key name".into());
    }
    result.push(key);
    Ok(result.join("+"))
}

struct SessionGuard {
    connection: zbus::Connection,
    path: OwnedObjectPath,
    closed: bool,
}
impl SessionGuard {
    async fn close(&mut self) {
        self.closed = true;
        if let Ok(proxy) = zbus::Proxy::new(
            &self.connection,
            portal::DESTINATION,
            self.path.clone(),
            "org.freedesktop.portal.Session",
        )
        .await
        {
            let _: Result<(), _> = proxy.call("Close", &()).await;
        }
    }
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let connection = self.connection.clone();
        let path = self.path.clone();
        // A cancelled/replaced binding must also close its compositor session.
        tauri::async_runtime::spawn(async move {
            if let Ok(proxy) = zbus::Proxy::new(
                &connection,
                portal::DESTINATION,
                path,
                "org.freedesktop.portal.Session",
            )
            .await
            {
                let _: Result<(), _> = proxy.call("Close", &()).await;
            }
        });
    }
}

#[async_trait::async_trait]
impl ShortcutOps for WaylandShortcuts {
    fn uses_portal() -> bool {
        true
    }
    fn suspend(&self, suspended: bool) {
        self.suspended.store(suspended, Ordering::Release);
    }
    fn shutdown(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.suspended.store(true, Ordering::Release);
        if let Ok(mut session) = self.session.lock()
            && let Some(stop) = session.take()
        {
            let _ = stop.send(());
        }
    }
    async fn bind(
        &self,
        bindings: Vec<ShortcutBinding>,
        handler: ShortcutHandler,
    ) -> Result<Vec<ShortcutRegistration>, String> {
        if super::session() != super::Session::Wayland {
            return Err(
                "Global shortcuts require a Wayland session and GlobalShortcuts portal".into(),
            );
        }
        let connection = portal::connect().await?;
        self.bind_on(connection, bindings, handler).await
    }
}

fn registrations(values: Values) -> Result<Vec<ShortcutRegistration>, String> {
    let bound: Vec<(String, Values)> = values
        .get("shortcuts")
        .ok_or("Missing bound shortcut list")?
        .try_clone()
        .map_err(error)?
        .try_into()
        .map_err(error)?;
    Ok(bound
        .into_iter()
        .map(|(id, values)| ShortcutRegistration {
            trigger: values
                .get("trigger_description")
                .and_then(|value| <&str>::try_from(value).ok())
                .unwrap_or("Desktop shortcut")
                .to_string(),
            id,
        })
        .collect())
}

impl WaylandShortcuts {
    async fn bind_on(
        &self,
        connection: zbus::Connection,
        bindings: Vec<ShortcutBinding>,
        handler: ShortcutHandler,
    ) -> Result<Vec<ShortcutRegistration>, String> {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(previous) = self.session.lock().map_err(error)?.take() {
            let _ = previous.send(());
        }
        if bindings.is_empty() {
            return Ok(Vec::new());
        }
        let proxy = portal::proxy(&connection, "org.freedesktop.portal.GlobalShortcuts").await?;
        let _: u32 = proxy
            .get_property("version")
            .await
            .map_err(|error| format!("GlobalShortcuts portal unavailable: {error}"))?;
        let create = portal::token();
        let session_token = portal::token();
        let options = HashMap::from([
            ("handle_token", Value::from(create.as_str())),
            ("session_handle_token", Value::from(session_token.as_str())),
        ]);
        let cancelled = || self.generation.load(Ordering::Acquire) != generation;
        let values = portal::request(
            &connection,
            &proxy,
            "CreateSession",
            &(options,),
            &create,
            &cancelled,
        )
        .await?;
        let path = values
            .get("session_handle")
            .and_then(|value| <&str>::try_from(value).ok())
            .ok_or("Missing global shortcut session handle")?;
        let sender = connection
            .unique_name()
            .ok_or("Missing bus name")?
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        if path != format!("{}/session/{sender}/{session_token}", portal::DESKTOP) {
            return Err("Unexpected shortcut session path".into());
        }
        let mut session = SessionGuard {
            connection: connection.clone(),
            path: OwnedObjectPath::try_from(path.to_string()).map_err(error)?,
            closed: false,
        };
        let mut activations = proxy.receive_signal("Activated").await.map_err(error)?;
        let mut changes = proxy
            .receive_signal("ShortcutsChanged")
            .await
            .map_err(error)?;
        let session_proxy = zbus::Proxy::new(
            &connection,
            portal::DESTINATION,
            session.path.clone(),
            "org.freedesktop.portal.Session",
        )
        .await
        .map_err(error)?;
        let mut closed = session_proxy
            .receive_signal("Closed")
            .await
            .map_err(error)?;
        let bind = portal::token();
        let descriptors = bindings
            .iter()
            .map(|binding| {
                Ok((
                    binding.id.clone(),
                    HashMap::from([
                        ("description", Value::from(binding.description.clone())),
                        (
                            "preferred_trigger",
                            Value::from(preferred(&binding.preferred)?),
                        ),
                    ]),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let values = portal::request(
            &connection,
            &proxy,
            "BindShortcuts",
            &(
                &session.path,
                descriptors,
                "",
                HashMap::from([("handle_token", Value::from(bind.as_str()))]),
            ),
            &bind,
            &cancelled,
        )
        .await?;
        let registrations: Vec<_> = registrations(values)?
            .into_iter()
            .filter(|entry| bindings.iter().any(|binding| binding.id == entry.id))
            .collect();
        if cancelled() {
            return Err("Shortcut binding was superseded".into());
        }
        let mut ids: Vec<_> = registrations.iter().map(|entry| entry.id.clone()).collect();
        let allowed: Vec<_> = bindings.into_iter().map(|binding| binding.id).collect();
        let suspended = self.suspended.clone();
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        {
            let mut current = self.session.lock().map_err(error)?;
            if cancelled() {
                return Err("Shortcut binding was superseded".into());
            }
            *current = Some(stop);
        }
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    signal = activations.next() => {
                        let Some(signal) = signal else { break; };
                        let Ok((path, id, _, options)): Result<(OwnedObjectPath, String, u64, Values), _> = signal.body().deserialize() else { continue; };
                        if path == session.path && ids.contains(&id) && !suspended.load(Ordering::Acquire) {
                            super::window::activation_token(options.get("activation_token").and_then(|value| <&str>::try_from(value).ok()).map(str::to_string));
                            handler(ShortcutEvent::Activated(id));
                        }
                    }
                    signal = changes.next() => {
                        let Some(signal) = signal else { break; };
                        let Ok((path, updated)): Result<(OwnedObjectPath, Vec<(String, Values)>), _> = signal.body().deserialize() else { continue; };
                        if path != session.path { continue; }
                        let updates: Vec<_> = updated.into_iter().filter(|(id, _)| allowed.contains(id)).map(|(id, values)| ShortcutRegistration {
                            trigger: values.get("trigger_description").and_then(|value| <&str>::try_from(value).ok()).unwrap_or("Desktop shortcut").to_string(), id,
                        }).collect();
                        ids = updates.iter().map(|entry| entry.id.clone()).collect();
                        handler(ShortcutEvent::Changed(updates));
                    }
                    _ = closed.next() => {
                        handler(ShortcutEvent::Closed("Desktop closed the global-shortcut permission session; retry to authorize again".into()));
                        break;
                    }
                }
            }
            session.close().await;
        });
        Ok(registrations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    struct FakeSession(Arc<AtomicU32>);
    #[zbus::interface(name = "org.freedesktop.portal.Session")]
    impl FakeSession {
        fn close(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    struct FakeShortcuts {
        closed: Arc<AtomicU32>,
        session: Arc<Mutex<Option<OwnedObjectPath>>>,
    }
    #[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
    impl FakeShortcuts {
        #[zbus(property, name = "version")]
        fn version(&self) -> u32 {
            1
        }
        async fn create_session(
            &self,
            options: Values,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            let sender = header
                .sender()
                .unwrap()
                .as_str()
                .trim_start_matches(':')
                .replace('.', "_");
            let session_token =
                <&str>::try_from(options.get("session_handle_token").unwrap()).unwrap();
            let session = format!("{}/session/{sender}/{session_token}", portal::DESKTOP);
            *self.session.lock().unwrap() =
                Some(OwnedObjectPath::try_from(session.clone()).unwrap());
            connection
                .object_server()
                .at(session.clone(), FakeSession(self.closed.clone()))
                .await?;
            respond(
                connection,
                &sender,
                &options,
                HashMap::from([("session_handle", Value::from(session))]),
            )
            .await
        }
        async fn bind_shortcuts(
            &self,
            _session: OwnedObjectPath,
            shortcuts: Vec<(String, Values)>,
            _parent: &str,
            options: Values,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            assert_eq!(shortcuts.len(), 1);
            assert_eq!(
                <&str>::try_from(shortcuts[0].1.get("preferred_trigger").unwrap()).unwrap(),
                "ALT+q"
            );
            let sender = header
                .sender()
                .unwrap()
                .as_str()
                .trim_start_matches(':')
                .replace('.', "_");
            let bound = vec![(
                "selection".to_string(),
                HashMap::from([("trigger_description", Value::from("Alt+Q"))]),
            )];
            respond(
                connection,
                &sender,
                &options,
                HashMap::from([("shortcuts", Value::new(bound))]),
            )
            .await
        }
    }

    async fn respond(
        connection: &zbus::Connection,
        sender: &str,
        options: &Values,
        values: HashMap<&str, Value<'_>>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let token = <&str>::try_from(options.get("handle_token").unwrap()).unwrap();
        let path =
            OwnedObjectPath::try_from(format!("{}/request/{sender}/{token}", portal::DESKTOP))
                .unwrap();
        connection
            .emit_signal(
                None::<&str>,
                path.clone(),
                "org.freedesktop.portal.Request",
                "Response",
                &(0u32, values),
            )
            .await?;
        Ok(path)
    }

    #[tokio::test]
    async fn portal_shortcut_binding_reports_actual_triggers_and_closes_on_shutdown() {
        let bus = super::super::test_bus::Bus::start();
        let server = bus.connection().await;
        server.request_name(portal::DESTINATION).await.unwrap();
        let closed = Arc::new(AtomicU32::new(0));
        let session = Arc::new(Mutex::new(None));
        server
            .object_server()
            .at(
                portal::DESKTOP,
                FakeShortcuts {
                    closed: closed.clone(),
                    session: session.clone(),
                },
            )
            .await
            .unwrap();
        let adapter = WaylandShortcuts::default();
        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        let bindings = vec![ShortcutBinding {
            id: "selection".into(),
            description: "Selection lookup".into(),
            preferred: "Alt+Q".into(),
        }];
        let result = adapter
            .bind_on(
                bus.connection().await,
                bindings,
                Arc::new(move |event| {
                    let _ = events.send(event);
                }),
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "selection");
        assert_eq!(result[0].trigger, "Alt+Q");
        let path = session.lock().unwrap().clone().unwrap();
        let activate = || async {
            server
                .emit_signal(
                    None::<&str>,
                    portal::DESKTOP,
                    "org.freedesktop.portal.GlobalShortcuts",
                    "Activated",
                    &(
                        path.clone(),
                        "selection",
                        0u64,
                        HashMap::<String, Value>::new(),
                    ),
                )
                .await
                .unwrap();
        };
        adapter.suspend(true);
        activate().await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(60), received.recv())
                .await
                .is_err()
        );
        adapter.suspend(false);
        activate().await;
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, ShortcutEvent::Activated(id) if id == "selection"));

        let changed = vec![(
            "selection",
            HashMap::from([("trigger_description", Value::from("Ctrl+Alt+Q"))]),
        )];
        server
            .emit_signal(
                None::<&str>,
                portal::DESKTOP,
                "org.freedesktop.portal.GlobalShortcuts",
                "ShortcutsChanged",
                &(path.clone(), changed),
            )
            .await
            .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(event, ShortcutEvent::Changed(entries) if entries[0].trigger == "Ctrl+Alt+Q")
        );
        server
            .emit_signal(
                None::<&str>,
                path,
                "org.freedesktop.portal.Session",
                "Closed",
                &(),
            )
            .await
            .unwrap();
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, ShortcutEvent::Closed(_)));
        adapter.shutdown();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while closed.load(Ordering::Acquire) == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(closed.load(Ordering::Acquire), 1);
    }
    #[test]
    fn translates_tauri_accelerators_without_shell_or_input_injection() {
        assert_eq!(preferred("Ctrl+Alt+Q").unwrap(), "CTRL+ALT+q");
        assert_eq!(preferred("Super+Shift+S").unwrap(), "LOGO+SHIFT+s");
        assert_eq!(preferred("Alt+Space").unwrap(), "ALT+space");
        assert_eq!(preferred("Ctrl+Enter").unwrap(), "CTRL+Return");
        assert_eq!(preferred("Alt+ArrowLeft").unwrap(), "ALT+Left");
        assert_eq!(preferred("Ctrl+Digit1").unwrap(), "CTRL+1");
        assert!(preferred("Bogus+Q").is_err());
        assert!(preferred("Ctrl++").is_err());
    }
}
