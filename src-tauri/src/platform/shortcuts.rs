//! Global-shortcut portal contract. Descriptors/callbacks contain no application
//! services or IPC labels. Platforms keep permission/session ownership here.

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ShortcutBinding {
    pub id: String,
    pub description: String,
    pub preferred: String,
}
#[derive(Debug, Clone)]
pub struct ShortcutRegistration {
    pub id: String,
    pub trigger: String,
}
pub enum ShortcutEvent {
    Activated(String),
    Changed(Vec<ShortcutRegistration>),
    Closed(String),
}
pub type ShortcutHandler = Arc<dyn Fn(ShortcutEvent) + Send + Sync>;

#[async_trait::async_trait]
pub trait ShortcutOps: Default + Send + Sync {
    fn uses_portal() -> bool;
    async fn bind(
        &self,
        bindings: Vec<ShortcutBinding>,
        handler: ShortcutHandler,
    ) -> Result<Vec<ShortcutRegistration>, String>;
    fn suspend(&self, suspended: bool);
    fn shutdown(&self) {}
}

#[derive(Default)]
pub struct NativeShortcuts;
#[async_trait::async_trait]
impl ShortcutOps for NativeShortcuts {
    fn uses_portal() -> bool {
        false
    }
    async fn bind(
        &self,
        _: Vec<ShortcutBinding>,
        _: ShortcutHandler,
    ) -> Result<Vec<ShortcutRegistration>, String> {
        Err("This platform uses native shortcut registration".into())
    }
    fn suspend(&self, _: bool) {}
}
