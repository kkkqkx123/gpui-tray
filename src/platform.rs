//! Platform backend selection. Each backend is compiled only for its target OS,
//! so a Linux build never pulls in `objc2` or `tray-icon`. Dispatch is static:
//! [`PlatformTrayBackend`] holds the one concrete backend of the current target
//! behind an `Arc` and forwards calls without a vtable.

use std::sync::Arc;

use gpui::App;

use crate::backend::TrayBackend;
use crate::error::TrayError;
use crate::icon::TrayIcon;
use crate::menu::ResolvedMenu;
use crate::options::TrayOptions;
use crate::runtime::RuntimeShared;

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
/// keeps [`crate::Tray`] cheaply cloneable. The bridge task is owned by the
/// inner backend and cancelled when it is dropped.
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
    /// The bridge task is created here and owned by the returned backend —
    /// never detached — so [`TrayBackend::shutdown`] can stop the event loop
    /// deterministically.
    pub(crate) fn new(
        cx: &mut App,
        options: &TrayOptions,
        shared: Arc<RuntimeShared>,
    ) -> Result<Self, TrayError> {
        #[cfg(target_os = "linux")]
        return Ok(Self {
            inner: Arc::new(linux::create_backend(cx, options, shared)?),
        });
        #[cfg(target_os = "macos")]
        return Ok(Self {
            inner: Arc::new(macos::create_backend(cx, options, shared)?),
        });
        #[cfg(target_os = "windows")]
        return Ok(Self {
            inner: Arc::new(windows::create_backend(cx, options, shared)?),
        });
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        return Ok(Self {
            inner: Arc::new(null::create_backend(cx, options, shared)?),
        });
    }
}

// Exactly one `inner` exists per build, so these forwards compile down to
// static dispatch on the concrete backend type.
impl TrayBackend for PlatformTrayBackend {
    fn is_available(&self) -> bool {
        self.inner.is_available()
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        self.inner.set_visible(visible)
    }

    fn set_icon(&self, icon: Option<TrayIcon>) -> Result<(), TrayError> {
        self.inner.set_icon(icon)
    }

    fn set_tooltip(&self, tooltip: Option<gpui::SharedString>) -> Result<(), TrayError> {
        self.inner.set_tooltip(tooltip)
    }

    fn set_title(&self, title: Option<gpui::SharedString>) -> Result<(), TrayError> {
        self.inner.set_title(title)
    }

    fn set_menu(&self, menu: &ResolvedMenu) -> Result<(), TrayError> {
        self.inner.set_menu(menu)
    }

    fn show_menu(&self) -> Result<(), TrayError> {
        self.inner.show_menu()
    }

    fn request_attention(&self, attention: bool) -> Result<(), TrayError> {
        self.inner.request_attention(attention)
    }

    fn shutdown(&self) {
        self.inner.shutdown();
    }
}
