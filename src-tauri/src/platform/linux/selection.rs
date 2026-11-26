use crate::platform::selection::{CaptureMethod, SelectionContent, SelectionOps, SelectionTarget};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, WebviewWindow};

#[derive(Default)]
pub struct LinuxSelection {
    input: Mutex<Option<super::remote_desktop::KeyboardSession>>,
    generation: AtomicU64,
    replacement: Mutex<Option<Armed>>,
}

struct Armed {
    snapshot: super::atspi::Snapshot,
    transport: ArmedTransport,
    target: SelectionTarget,
}
struct ArmedTransport {
    ticket: u64,
    proxy: zbus::Proxy<'static>,
}
impl Drop for Armed {
    fn drop(&mut self) {
        let proxy = self.transport.proxy.clone();
        let ticket = self.transport.ticket;
        tauri::async_runtime::spawn(async move {
            let _: Result<(), _> = proxy.call("CancelReplacement", &(ticket,)).await;
        });
    }
}

impl LinuxSelection {
    /// Explicitly authorized, target-pinned Ctrl+V transport. This primitive is
    /// intentionally not called by text replacement: clipboard handoff is a
    /// separate user-visible policy that must preserve every offered MIME type.
    pub fn paste_shortcut_for(&self, process: u32) -> Result<(), String> {
        if process == 0 {
            return Err("A nonzero input target PID is required".into());
        }
        let proxy = super::gestures::remote_proxy()?;
        let input = self.input.lock().map_err(|error| error.to_string())?;
        let session = input
            .as_ref()
            .ok_or("RemoteDesktop keyboard authorization is not active")?;
        tauri::async_runtime::block_on(session.paste_shortcut(&proxy, process))
    }

    fn arm(&self) -> Result<(SelectionContent, SelectionTarget), String> {
        let mut armed = self.replacement.lock().map_err(super::portal::error)?;
        armed.take();
        let proxy = super::gestures::plugin_proxy()?;
        let snapshot = super::atspi::snapshot()?;
        if !snapshot.editable {
            return Err("The selected range is not in an editable field".into());
        }
        let ticket: u64 = tauri::async_runtime::block_on(async {
            proxy
                .call("ArmReplacement", &(snapshot.process, &snapshot.text))
                .await
        })
        .map_err(super::portal::error)
        .unwrap_or(0);
        if ticket == 0 {
            return Err("KWin text-input replacement is unavailable; RemoteDesktop paste transport is not used without explicit clipboard-handoff approval".into());
        }
        let transport = ArmedTransport { ticket, proxy };
        let identity = ticket;
        let handle = usize::try_from(identity).map_err(super::portal::error)?;
        let target = SelectionTarget {
            process: snapshot.process,
            window: handle,
            focus: handle,
        };
        let content = SelectionContent {
            text: snapshot.text.clone(),
            bounds: None,
        };
        *armed = Some(Armed {
            snapshot,
            transport,
            target: target.clone(),
        });
        Ok((content, target))
    }

    fn commit(
        &self,
        target: &SelectionTarget,
        expected: &str,
        replacement: &str,
    ) -> Result<(), String> {
        let armed = self
            .replacement
            .lock()
            .map_err(super::portal::error)?
            .take()
            .ok_or("Replacement expired; select the text again")?;
        if armed.target != *target
            || armed.snapshot.text.trim() != expected
            || replacement.contains('\0')
        {
            return Err("Replacement did not match its original selection".into());
        }
        let current = super::atspi::snapshot()?;
        if current != armed.snapshot {
            return Err("The focused field or exact selection changed; no text was sent".into());
        }
        let committed: bool = tauri::async_runtime::block_on(async {
            armed
                .transport
                .proxy
                .call(
                    "CommitReplacement",
                    &(
                        current.process,
                        armed.transport.ticket,
                        &current.text,
                        replacement,
                    ),
                )
                .await
        })
        .map_err(super::portal::error)?;
        if !committed {
            return Err("KWin rejected an expired or changed selection; no text was sent".into());
        }
        Ok(())
    }
}
impl SelectionOps for LinuxSelection {
    fn authorize_input(&self) -> Result<(), String> {
        let generation = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        // Replacing a session closes its previous permission scope first.
        {
            let mut input = self.input.lock().map_err(|error| error.to_string())?;
            if self.generation.load(Ordering::Acquire) != generation {
                return Err("Input authorization was superseded".into());
            }
            input.take();
        }
        let session = super::remote_desktop::KeyboardSession::authorize(&|| {
            self.generation.load(Ordering::Acquire) != generation
        })?;
        if let Err(error) = super::gestures::set_remote_input_authorized(true) {
            drop(session);
            return Err(error);
        }
        let mut input = self.input.lock().map_err(|error| error.to_string())?;
        if self.generation.load(Ordering::Acquire) != generation {
            drop(input);
            let _ = super::gestures::set_remote_input_authorized(false);
            return Err("Input authorization was superseded".into());
        }
        *input = Some(session);
        Ok(())
    }
    fn revoke_input(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let _ = super::gestures::set_remote_input_authorized(false);
        if let Ok(mut input) = self.input.lock() {
            input.take();
        }
    }
    fn input_authorized(&self) -> bool {
        let authorized = self
            .input
            .lock()
            .is_ok_and(|input| input.as_ref().is_some_and(|session| session.active()));
        if !authorized {
            let _ = super::gestures::set_remote_input_authorized(false);
        }
        authorized
    }
    fn requires_focus(&self) -> bool {
        true // The GTK fallback needs focus.
    }
    fn method(&self) -> CaptureMethod {
        CaptureMethod::PrimarySelection
    }
    fn read_selection(&self) -> Result<SelectionContent, String> {
        if super::session() != super::Session::Wayland {
            return Err("A Wayland session is required".into());
        }
        Ok(SelectionContent {
            text: super::clipboard::read(true)?,
            bounds: None,
        })
    }
    fn read_before_activation(&self) -> Option<SelectionContent> {
        let snapshot = super::atspi::snapshot().ok()?;
        Some(SelectionContent {
            text: snapshot.text,
            bounds: None,
        })
    }
    fn read_compositor_selection(&self, process: u32) -> Result<SelectionContent, String> {
        let snapshot = super::atspi::snapshot()?;
        if snapshot.process != process {
            return Err("Source process changed".into());
        }
        Ok(SelectionContent {
            text: snapshot.text,
            bounds: None,
        })
    }
    fn copy_to_clipboard(&self) -> Result<(), String> {
        Ok(())
    }
    fn arm_replacement(&self) -> Option<Result<(SelectionContent, SelectionTarget), String>> {
        Some(self.arm())
    }
    fn foreground(&self) -> Option<SelectionTarget> {
        None
    }
    fn replace(
        &self,
        _: &AppHandle,
        _: &WebviewWindow,
        target: &SelectionTarget,
        expected: &str,
        replacement: &str,
    ) -> Result<(), String> {
        self.commit(target, expected, replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::gestures::GestureSource;

    #[test]
    #[ignore = "requires the isolated KWin/Qt editor; set RUST_REPLACEMENT_PROBE in native/smoke.sh"]
    fn real_kwin_replacement() {
        let (sender, _received) = std::sync::mpsc::sync_channel(16);
        let _observer = super::super::gestures::KWinGestures
            .observe(sender)
            .unwrap();
        let start = std::time::Instant::now();
        while !super::super::gestures::replacement_available() {
            assert!(
                start.elapsed() < std::time::Duration::from_secs(3),
                "Native plugin did not authorize test executable"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let service = LinuxSelection::default();
        let (content, target) = service
            .arm()
            .expect("Could not bind AT-SPI selection to KWin ticket");
        assert_eq!(content.text, "original");
        service
            .commit(&target, "original", "译文🙂")
            .expect("Native text commit failed");
        assert!(service.commit(&target, "original", "replayed").is_err());
        eprintln!(
            "Real Rust/KWin: AT-SPI target armed, validated, committed and consumed exactly once"
        );
    }
}
