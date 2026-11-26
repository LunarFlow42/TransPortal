//! XDG portal request protocol. Subscribe before sending the method to avoid
//! fast-response races, validate the returned handle, close on timeout/cancel.

use futures_util::StreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Value};

pub(super) const DESTINATION: &str = "org.freedesktop.portal.Desktop";
pub(super) const DESKTOP: &str = "/org/freedesktop/portal/desktop";
pub(super) type Values = HashMap<String, OwnedValue>;
pub(super) fn error(value: impl std::fmt::Display) -> String {
    value.to_string()
}

pub(super) fn token() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "transportal_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub(super) async fn proxy(
    connection: &zbus::Connection,
    interface: &'static str,
) -> Result<zbus::Proxy<'static>, String> {
    zbus::Proxy::new(connection, DESTINATION, DESKTOP, interface)
        .await
        .map_err(error)
}

pub(super) async fn connect() -> Result<zbus::Connection, String> {
    tokio::time::timeout(Duration::from_secs(10), async {
        zbus::connection::Builder::session()
            .map_err(error)?
            .method_timeout(Duration::from_secs(10))
            .build()
            .await
            .map_err(error)
    })
    .await
    .map_err(|_| "Session bus connection timed out".to_string())?
}

pub(super) async fn request<B: Serialize + DynamicType>(
    connection: &zbus::Connection,
    portal: &zbus::Proxy<'_>,
    method: &str,
    body: &B,
    token: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Values, String> {
    let sender = connection
        .unique_name()
        .ok_or("Portal connection has no bus name")?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("{DESKTOP}/request/{sender}/{token}");
    let pending = zbus::Proxy::new(
        connection,
        DESTINATION,
        path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .await
    .map_err(error)?;
    let mut responses = pending.receive_signal("Response").await.map_err(error)?;
    let result = async {
        let handle: OwnedObjectPath = tokio::time::timeout(Duration::from_secs(15), portal.call(method, body))
            .await.map_err(|_| "Portal method timed out".to_string())?.map_err(error)?;
        if handle.as_str() != path { return Err("Portal returned an unexpected request handle".into()); }
        let timeout = tokio::time::sleep(Duration::from_secs(120));
        tokio::pin!(timeout);
        let mut poll = tokio::time::interval(Duration::from_millis(50));
        loop {
            tokio::select! {
                _ = &mut timeout => return Err("Portal permission request timed out".into()),
                _ = poll.tick() => if cancelled() { return Err("Portal request cancelled".into()); },
                response = responses.next() => {
                    let response = response.ok_or("Portal disconnected before responding")?;
                    let (code, values): (u32, Values) = response.body().deserialize().map_err(error)?;
                    return match code {
                        0 => Ok(values),
                        1 => Err("Portal request cancelled by the user".into()),
                        _ => Err("Portal request denied or failed; check the desktop portal backend".into()),
                    };
                }
            }
        }
    }.await;
    if result.is_err() {
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            pending.call::<_, _, ()>("Close", &()),
        )
        .await;
    }
    result
}

fn decode_image(uri: &str) -> Result<image::RgbaImage, String> {
    let uri = url::Url::parse(uri).map_err(error)?;
    if uri.scheme() != "file" || uri.host_str().is_some_and(|host| host != "localhost") {
        return Err("Screenshot portal returned a non-local file URI".into());
    }
    if uri.query().is_some() || uri.fragment().is_some() {
        return Err("Screenshot URI must not contain query or fragment data".into());
    }
    let path = uri
        .to_file_path()
        .map_err(|_| "Invalid screenshot file URI")?;
    let file = std::fs::File::open(path).map_err(error)?;
    let metadata = file.metadata().map_err(error)?;
    if !metadata.is_file() || metadata.len() > 64_000_000 {
        return Err("Portal screenshot exceeds the file size limit".into());
    }
    let mut reader = image::ImageReader::new(std::io::BufReader::new(file))
        .with_guessed_format()
        .map_err(error)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    limits.max_alloc = Some(128_000_000);
    reader.limits(limits);
    let image = reader.decode().map_err(error)?.into_rgba8();
    crate::platform::desktop::ScreenRect {
        x: 0,
        y: 0,
        width: image.width(),
        height: image.height(),
    }
    .validate()?;
    // Do not delete a portal-owned/document-exported file: it is not ours.
    Ok(image)
}

pub(super) fn screenshot(
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<image::RgbaImage, String> {
    tauri::async_runtime::block_on(async {
        if cancelled() {
            return Err("Screenshot cancelled".into());
        }
        let connection = connect().await?;
        screenshot_on(&connection, cancelled).await
    })
}

async fn screenshot_on(
    connection: &zbus::Connection,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<image::RgbaImage, String> {
    let portal = proxy(connection, "org.freedesktop.portal.Screenshot").await?;
    let version: u32 = portal.get_property("version").await.map_err(error)?;
    if version < 2 {
        return Err(
            "Screenshot portal version 2 or later is required for interactive capture".into(),
        );
    }
    let token = token();
    let mut options = HashMap::from([
        ("handle_token", Value::from(token.as_str())),
        ("interactive", Value::from(true)),
        ("modal", Value::from(false)),
    ]);
    if version >= 3 {
        let targets: u32 = portal
            .get_property("AvailableTargets")
            .await
            .map_err(error)?;
        if targets & 4 != 0 {
            options.insert("target", Value::from(4u32));
        }
    }
    let values = request(
        connection,
        &portal,
        "Screenshot",
        &("", options),
        &token,
        cancelled,
    )
    .await?;
    if cancelled() {
        return Err("Screenshot cancelled".into());
    }
    let uri = values
        .get("uri")
        .and_then(|value| <&str>::try_from(value).ok())
        .ok_or("Screenshot response has no file URI")?;
    decode_image(uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    struct FakeRequest(Arc<AtomicU32>);
    #[zbus::interface(name = "org.freedesktop.portal.Request")]
    impl FakeRequest {
        fn close(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    struct FakeScreenshot {
        uri: String,
        code: u32,
        closed: Arc<AtomicU32>,
        requests: Arc<AtomicU32>,
        version: u32,
    }
    #[zbus::interface(name = "org.freedesktop.portal.Screenshot")]
    impl FakeScreenshot {
        #[zbus(property, name = "version")]
        fn version(&self) -> u32 {
            self.version
        }
        #[zbus(property)]
        fn available_targets(&self) -> u32 {
            5
        }
        async fn screenshot(
            &self,
            _parent: &str,
            options: Values,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) -> zbus::fdo::Result<OwnedObjectPath> {
            assert_eq!(
                options
                    .get("interactive")
                    .and_then(|value| bool::try_from(value).ok()),
                Some(true)
            );
            assert_eq!(
                options
                    .get("target")
                    .and_then(|value| u32::try_from(value).ok()),
                (self.version >= 3).then_some(4)
            );
            self.requests.fetch_add(1, Ordering::Relaxed);
            let sender = header
                .sender()
                .unwrap()
                .as_str()
                .trim_start_matches(':')
                .replace('.', "_");
            let token = options
                .get("handle_token")
                .and_then(|value| <&str>::try_from(value).ok())
                .unwrap();
            let path =
                OwnedObjectPath::try_from(format!("{DESKTOP}/request/{sender}/{token}")).unwrap();
            connection
                .object_server()
                .at(path.clone(), FakeRequest(self.closed.clone()))
                .await?;
            if self.code != 99 {
                let values = HashMap::from([("uri", Value::from(self.uri.as_str()))]);
                // Deliberately emit before the method returns to exercise the race.
                connection
                    .emit_signal(
                        None::<&str>,
                        path.clone(),
                        "org.freedesktop.portal.Request",
                        "Response",
                        &(self.code, values),
                    )
                    .await?;
            }
            Ok(path)
        }
    }

    async fn fixture(
        code: u32,
        uri: String,
        version: u32,
    ) -> (
        super::super::test_bus::Bus,
        zbus::Connection,
        zbus::Connection,
        Arc<AtomicU32>,
    ) {
        let bus = super::super::test_bus::Bus::start();
        let server = bus.connection().await;
        server.request_name(DESTINATION).await.unwrap();
        let closed = Arc::new(AtomicU32::new(0));
        server
            .object_server()
            .at(
                DESKTOP,
                FakeScreenshot {
                    uri,
                    code,
                    closed: closed.clone(),
                    requests: Arc::new(AtomicU32::new(0)),
                    version,
                },
            )
            .await
            .unwrap();
        let client = bus.connection().await;
        (bus, server, client, closed)
    }

    #[tokio::test]
    async fn screenshot_request_handles_an_immediate_response_on_a_private_bus() {
        let path = std::env::temp_dir().join(format!("{}.png", token()));
        image::RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let (_bus, _server, client, closed) =
            fixture(0, url::Url::from_file_path(&path).unwrap().to_string(), 3).await;
        let image = screenshot_on(&client, &|| false).await.unwrap();
        assert_eq!(image.dimensions(), (4, 3));
        assert_eq!(closed.load(Ordering::Relaxed), 0);
        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn cancellation_and_denial_close_the_portal_request() {
        for code in [1, 2, 99] {
            let (_bus, _server, client, closed) =
                fixture(code, "file:///unused.png".into(), 3).await;
            let result = screenshot_on(&client, &|| code == 99).await;
            assert!(result.is_err());
            assert_eq!(closed.load(Ordering::Relaxed), 1);
        }
    }
    #[tokio::test]
    async fn screenshot_v2_uses_interactive_without_v3_options_and_v1_is_rejected() {
        let (_bus, _server, client, closed) = fixture(1, "file:///unused.png".into(), 2).await;
        assert!(
            screenshot_on(&client, &|| false)
                .await
                .unwrap_err()
                .contains("cancelled")
        );
        assert_eq!(closed.load(Ordering::Relaxed), 1);
        let (_bus, _server, client, closed) = fixture(0, "file:///unused.png".into(), 1).await;
        assert!(
            screenshot_on(&client, &|| false)
                .await
                .unwrap_err()
                .contains("version 2")
        );
        assert_eq!(closed.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn remote_and_malformed_screenshot_uris_are_never_fetched() {
        for uri in [
            "https://example.com/image.png",
            "file://remotehost/private.png",
            "not a uri",
        ] {
            assert!(decode_image(uri).is_err());
        }
    }
    #[test]
    fn decodes_local_screenshots_without_deleting_portal_files() {
        let path = std::env::temp_dir().join(format!("{}.png", token()));
        image::RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let uri = url::Url::from_file_path(&path).unwrap();
        assert_eq!(decode_image(uri.as_str()).unwrap().dimensions(), (4, 3));
        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
    }
}
