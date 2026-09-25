pub trait IntegrityPlatform {
    fn enforce_release_hash(&self) -> bool;
    fn show_failure(&self, message: &str);
}

#[cfg(target_os = "windows")]
mod windows;

pub fn platform() -> Box<dyn IntegrityPlatform> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsIntegrity)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedIntegrity)
    }
}

#[cfg(not(target_os = "windows"))]
struct UnsupportedIntegrity;

#[cfg(not(target_os = "windows"))]
impl IntegrityPlatform for UnsupportedIntegrity {
    fn enforce_release_hash(&self) -> bool {
        false
    }
    fn show_failure(&self, message: &str) {
        eprintln!("{message}");
    }
}
