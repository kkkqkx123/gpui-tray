//! The platform-agnostic backend trait every tray implementation satisfies.

use gpui::SharedString;

use crate::error::TrayError;
use crate::icon::TrayIcon;
use crate::menu::ResolvedMenu;

/// A platform tray implementation.
///
/// [`crate::Tray`] forwards to the concrete backend of the current platform
/// through [`crate::platform::PlatformTrayBackend`]. The native handles a
/// backend owns belong to the thread that created them, so a backend is not
/// `Send`. Every mutation is fallible: backends report failures instead of
/// silently succeeding.
pub trait TrayBackend {
    /// Whether the backend registered successfully.
    fn is_available(&self) -> bool;

    /// Show or hide the tray icon. Must not affect availability.
    fn set_visible(&self, visible: bool) -> Result<(), TrayError>;

    /// Set the tray icon (or clear it with `None`).
    fn set_icon(&self, icon: Option<TrayIcon>) -> Result<(), TrayError>;

    /// Set the hover tooltip text (or clear it with `None`).
    fn set_tooltip(&self, tooltip: Option<SharedString>) -> Result<(), TrayError>;

    /// Set the text shown next to the icon where the platform supports it
    /// (Linux, macOS); unsupported platforms report
    /// [`TrayError::UnsupportedPlatform`].
    fn set_title(&self, title: Option<SharedString>) -> Result<(), TrayError>;

    /// Replace the menu with a resolved snapshot. Dispatch handlers are
    /// already stored in the runtime; backends only receive render data.
    fn set_menu(&self, menu: &ResolvedMenu) -> Result<(), TrayError>;

    /// Open the menu programmatically, where the platform supports it.
    fn show_menu(&self) -> Result<(), TrayError>;

    /// Mark the icon as requesting attention, where the platform supports it
    /// (Linux `NeedsAttention`).
    fn request_attention(&self, attention: bool) -> Result<(), TrayError>;

    /// Remove the native icon and stop the backend service. Called by
    /// [`crate::Tray::remove`]; there is no default implementation so every
    /// backend must decide how to tear itself down.
    fn shutdown(&self);
}
