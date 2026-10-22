//! The platform-agnostic backend trait every tray implementation satisfies.

use gpui::SharedString;

use crate::icon::TrayIconData;
use crate::menu::TrayMenu;
use crate::LeftClick;

/// A platform tray implementation.
///
/// [`Tray`] forwards to the concrete backend of the current platform through
/// [`crate::PlatformTrayBackend`], so implementations must be `Send + Sync`.
pub trait TrayBackend: Send + Sync {
    /// Whether a working tray is present on this platform/session.
    fn is_available(&self) -> bool;

    /// Show or hide the tray icon.
    fn set_visible(&self, visible: bool);

    /// Set the tray icon from raw RGBA bitmap data (or clear it with `None`).
    fn set_icon(&self, icon: Option<TrayIconData>);

    /// Set the hover tooltip text.
    fn set_tooltip(&self, tooltip: Option<SharedString>);

    /// Replace the entire menu.
    fn set_menu(&self, menu: TrayMenu);

    /// Configure what a tray icon activation (left click) does.
    fn set_left_click_action(&self, behavior: LeftClick);

    /// Convenience used by backends to nudge the native presentation after a state
    /// change. The default is a no-op.
    fn shutdown(&self) {}
}
