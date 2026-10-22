//! Platform backend selection. Each backend is compiled only for its target OS,
//! so a Linux build never pulls in `objc2` or `tray-icon`. Dispatch is static:
//! [`PlatformTrayBackend`] holds the one concrete backend of the current target
//! behind an `Arc` and forwards calls without a vtable.

use std::sync::Arc;

use gpui::App;

use crate::backend::TrayBackend;
use crate::icon::TrayIconData;
use crate::menu::TrayMenu;
use crate::LeftClick;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
// Fallback for any platform we do not implement: a no-op backend that always
// reports itself unavailable so the host can degrade gracefully.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod null;

/// The concrete tray backend of the current target platform.
///
/// Exactly one inner backend exists per build, so every call forwards through
/// `Arc<T>` with static dispatch — no trait object is involved. The `Arc`
/// keeps [`crate::Tray`] cheaply cloneable and `Send + Sync`.
#[derive(Clone)]
pub struct PlatformTrayBackend {
    #[cfg(target_os = "linux")]
    inner: Arc<linux::LinuxBackend>,
    #[cfg(target_os = "macos")]
    inner: Arc<macos::MacBackend>,
    #[cfg(target_os = "windows")]
    inner: Arc<windows::WindowsBackend>,
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    inner: Arc<null::NullBackend>,
}

impl PlatformTrayBackend {
    /// Creates the backend of the current platform.
    ///
    /// Never blocks on desktop startup; if the platform tray is unavailable the
    /// backend reports that through [`TrayBackend::is_available`] and the host
    /// degrades gracefully.
    pub fn new(cx: &mut App) -> anyhow::Result<Self> {
        #[cfg(target_os = "linux")]
        return Ok(Self {
            inner: Arc::new(linux::create_backend(cx)?),
        });
        #[cfg(target_os = "macos")]
        return Ok(Self {
            inner: Arc::new(macos::create_backend(cx)?),
        });
        #[cfg(target_os = "windows")]
        return Ok(Self {
            inner: Arc::new(windows::create_backend(cx)?),
        });
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        return Ok(Self {
            inner: Arc::new(null::create_backend(cx)?),
        });
    }
}

// Exactly one `inner` exists per build, so these forwards compile down to
// static dispatch on the concrete backend type.
impl TrayBackend for PlatformTrayBackend {
    fn is_available(&self) -> bool {
        self.inner.is_available()
    }

    fn set_visible(&self, visible: bool) {
        self.inner.set_visible(visible);
    }

    fn set_icon(&self, icon: Option<TrayIconData>) {
        self.inner.set_icon(icon);
    }

    fn set_tooltip(&self, tooltip: Option<gpui::SharedString>) {
        self.inner.set_tooltip(tooltip);
    }

    fn set_menu(&self, menu: TrayMenu) {
        self.inner.set_menu(menu);
    }

    fn set_left_click_action(&self, behavior: LeftClick) {
        self.inner.set_left_click_action(behavior);
    }
}
