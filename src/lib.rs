//! # gpui-tray
//!
//! System tray / menu-bar icon support for [GPUI](https://gpui.rs) applications.
//!
//! The crate provides a small, cross-platform [`Tray`] handle. Menu items carry
//! either a GPUI [`Action`](gpui::Action) (dispatched through the normal GPUI
//! action pipeline) or a plain callback, so tray interaction looks exactly like
//! the app menu / keybinding paths already in GPUI.
//!
//! ## Backends
//!
//! | Platform | Backend                              |
//! | -------- | ------------------------------------ |
//! | Linux    | `ksni` (StatusNotifierItem / DBus)   |
//! | macOS    | `objc2` NSStatusItem (native)        |
//! | Windows  | `tray-icon` (Shell_NotifyIcon)       |
//!
//! If the platform tray is unavailable (e.g. GNOME without the AppIndicator
//! extension) [`Tray::is_available`] returns `false` and the application starts
//! normally — the tray never blocks startup.
//!
//! ## GPUI flavors
//!
//! Select exactly one flavor: `gpui-kit` (GPUI Kit's `gpui-pre` types) or
//! `gpui-fast` (GPUI Fast). Disable default features when selecting an
//! alternative. The selected flavor must be the GPUI the application itself
//! builds against, because every operation takes that flavor's `gpui::App`.
//!
//! ## Example
//!
//! ```ignore
//! use gpui_kit::*;
//! use gpui_tray::*;
//!
//! actions!(demo, [ShowWindow, Quit]);
//!
//! fn main() {
//!     gpui_kit::application().run(|cx| {
//!         gpui_kit::init(cx);
//!         let tray = Tray::new(cx).expect("tray");
//!         tray.set_tooltip(Some("Demo"));
//!         tray.set_menu(tray_menu(|m| {
//!             m.action(ShowWindow, "Show Window");
//!             m.separator();
//!             m.action(Quit, "Quit");
//!         }));
//!     });
//! }
//! ```

#[cfg(all(feature = "gpui-kit", feature = "gpui-fast"))]
compile_error!(
    "features `gpui-kit` and `gpui-fast` are mutually exclusive; use default-features = false and select exactly one backend"
);

#[cfg(not(any(feature = "gpui-kit", feature = "gpui-fast")))]
compile_error!("enable exactly one of the `gpui-kit` or `gpui-fast` features");

// Both flavors arrive under a renamed dependency, so the selected one takes
// the name `gpui` here.
#[cfg(all(feature = "gpui-kit", not(feature = "gpui-fast")))]
extern crate gpui_kit_backend as gpui;
#[cfg(all(feature = "gpui-fast", not(feature = "gpui-kit")))]
extern crate gpui_fast_backend as gpui;

pub mod backend;
pub mod event;
pub mod icon;
pub mod menu;
pub mod platform;

pub use backend::TrayBackend;
pub use event::TrayEvent;
pub use icon::{TrayIconData, TrayIconRef};
pub use menu::{
    tray_menu, AnyAction, MenuItemId, TrayItem, TrayMenu, TrayMenuBuilder, TrayMenuItem, TraySubmenu,
};
pub use platform::PlatformTrayBackend;

use std::sync::Arc;

use gpui::{App, SharedString};

/// Behavior triggered when the tray icon is activated (typically a left click).
pub enum LeftClick {
    /// Open the tray menu. On Linux the desktop shell draws the menu (it is also
    /// reachable via right click); on macOS/Windows the left click opens it.
    Menu,
    /// Dispatch an action when the tray icon is activated.
    Action(AnyAction),
}

impl Clone for LeftClick {
    fn clone(&self) -> Self {
        match self {
            LeftClick::Menu => LeftClick::Menu,
            LeftClick::Action(a) => LeftClick::Action(Arc::clone(a)),
        }
    }
}

/// A handle to the system tray.
///
/// Holds the concrete backend of the current platform (behind an `Arc`), so
/// calls dispatch statically. Cheaply cloneable and `Send`; safe to share
/// across the application. Must be created inside `application().run(|cx| …)`
/// on the main thread.
pub struct Tray {
    backend: PlatformTrayBackend,
}

impl Clone for Tray {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
        }
    }
}

impl Tray {
    /// Create the tray on the current platform.
    ///
    /// Never blocks on desktop startup. If the platform tray is unavailable,
    /// [`Tray::is_available`] returns `false` and the app runs normally.
    pub fn new(cx: &mut App) -> anyhow::Result<Self> {
        Ok(Self {
            backend: PlatformTrayBackend::new(cx)?,
        })
    }

    /// Whether a working tray is present on this platform/session.
    pub fn is_available(&self) -> bool {
        self.backend.is_available()
    }

    /// Show or hide the tray icon.
    pub fn set_visible(&self, visible: bool) {
        self.backend.set_visible(visible);
    }

    /// Set the tray icon from raw RGBA bitmap data (or clear it with `None`).
    pub fn set_icon(&self, icon: Option<TrayIconData>) {
        self.backend.set_icon(icon);
    }

    /// Set the hover tooltip text.
    pub fn set_tooltip(&self, tooltip: Option<SharedString>) {
        self.backend.set_tooltip(tooltip);
    }

    /// Replace the entire menu.
    pub fn set_menu(&self, menu: TrayMenu) {
        self.backend.set_menu(menu);
    }

    /// Configure what a tray icon activation (left click) does.
    pub fn set_left_click_action(&self, behavior: LeftClick) {
        self.backend.set_left_click_action(behavior);
    }
}
