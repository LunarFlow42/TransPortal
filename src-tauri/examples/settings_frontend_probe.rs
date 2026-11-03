//! Exercises the bundled React settings page in an isolated WebView/profile.
//! Run scripts/window/settings-probe.ps1 -Frontend [-CorruptOpacity | -ThemeError].

use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::Manager;
use tauri_plugin_store::StoreExt;

#[derive(Default, Clone, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DocumentSnapshot {
    children: usize,
    settings: bool,
    failed: bool,
    alpha: String,
}

#[tauri::command]
fn probe_document(snapshot: DocumentSnapshot, state: tauri::State<Mutex<DocumentSnapshot>>) {
    *state.lock().unwrap() = snapshot;
}

#[allow(deprecated)]
fn pump(app: &mut tauri::App, duration: Duration) {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        app.run_iteration(|_, _| {});
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() -> Result<(), String> {
    let corrupt = std::env::args().any(|arg| arg == "--corrupt-opacity");
    let theme_error = std::env::args().any(|arg| arg == "--theme-error");
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.transportal.settings-frontend-probe".into();
    context.config_mut().build.dev_url = None;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let profile = root.join("target/settings-frontend-probe");
    context.config_mut().app.app_directories_override =
        Some(tauri::utils::config::AppDirectoriesOverride::Root(profile));
    let mut config = context
        .config_mut()
        .app
        .windows
        .iter()
        .find(|window| window.label == "settings")
        .ok_or("missing settings config")?
        .clone();
    context.config_mut().app.windows.clear();
    config.url =
        tauri::WebviewUrl::External("probe://localhost/index.html#/settings".parse().unwrap());
    let dist = root.parent().unwrap().join("dist");
    let document_handler: Box<dyn Fn(tauri::ipc::Invoke) -> bool + Send + Sync> =
        Box::new(tauri::generate_handler![probe_document]);
    let mut app = tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(Mutex::new(DocumentSnapshot::default()))
        .register_uri_scheme_protocol("probe", move |_, request| {
            let path = request.uri().path().trim_start_matches('/');
            let relative = std::path::Path::new(path);
            if relative.components().any(|part| !matches!(part, std::path::Component::Normal(_))) {
                return tauri::http::Response::builder().status(400).body(Vec::new()).unwrap();
            }
            let mime = match relative.extension().and_then(|ext| ext.to_str()) {
                Some("js") => "text/javascript",
                Some("css") => "text/css",
                Some("png") => "image/png",
                Some("svg") => "image/svg+xml",
                _ => "text/html; charset=utf-8",
            };
            match std::fs::read(dist.join(relative)) {
                Ok(body) => tauri::http::Response::builder().header("Content-Type", mime).body(body).unwrap(),
                Err(_) => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
            }
        })
        .invoke_handler(move |invoke| {
            if invoke.message.command() == "probe_document" {
                return document_handler(invoke);
            }
            // No real app services, shortcuts, clipboard or model downloads.
            match invoke.message.command() {
                "get_ocr_config" => invoke.resolver.resolve(serde_json::json!({"engine":"windows","language":null})),
                "get_shortcut_status" => invoke.resolver.resolve(serde_json::json!({"actions":[]})),
                _ => invoke.resolver.reject("Service unavailable in the isolated frontend probe"),
            }
            true
        })
        .setup(move |app| {
            let store = app.store_builder("settings.json").build()?;
            store.clear();
            store.set("overlayOpacity", if corrupt { serde_json::json!("invalid-opacity") } else { serde_json::json!(0.72) });
            let mut builder = tauri::WebviewWindowBuilder::from_config(app, &config)?;
            if theme_error {
                builder = builder.initialization_script("window.matchMedia = () => { throw new Error('synthetic global theme failure'); };");
            }
            builder.build()?;
            Ok(())
        })
        .build(context).map_err(|error| error.to_string())?;
    pump(&mut app, Duration::from_millis(500));
    let window = app
        .get_webview_window("settings")
        .ok_or("missing settings window")?;
    transportal_lib::open_or_focus_settings(app.handle())?;
    // Inspect actual DOM and styles after React effects/bootstrap have settled.
    pump(&mut app, Duration::from_secs(3));
    window
        .eval(
            r#"window.__TAURI_INTERNALS__.invoke('probe_document', {snapshot: {
        children: document.getElementById('root')?.childElementCount ?? 0,
        settings: !!document.querySelector('.window-root'),
        failed: !!document.querySelector('[role="alert"] button'),
        alpha: document.documentElement.style.getPropertyValue('--surface-alpha')
    }});"#,
        )
        .map_err(|error| error.to_string())?;
    pump(&mut app, Duration::from_millis(300));
    let result = app
        .state::<Mutex<DocumentSnapshot>>()
        .lock()
        .unwrap()
        .clone();
    println!("React settings probe: {result:?}");
    let passed = result.children > 0
        && result.settings
        && if theme_error {
            result.failed
        } else {
            !result.failed && result.alpha == "0.720"
        };
    window.destroy().map_err(|error| error.to_string())?;
    pump(&mut app, Duration::from_millis(100));
    app.cleanup_before_exit();
    if !passed {
        return Err("Settings React page is blank, failed, or has invalid opacity".into());
    }
    Ok(())
}
