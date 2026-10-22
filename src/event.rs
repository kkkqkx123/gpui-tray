//! Events produced by the tray backends and bridged to the GPUI main thread.

use crate::menu::MenuItemId;

/// An event originating from the native tray.
#[derive(Debug, Clone)]
pub enum TrayEvent {
    /// Tray icon activated (typically a left click). Coordinates are screen
    /// pixels, useful as a hint for where to place a window.
    IconActivated {
        /// Horizontal screen position of the activation, in pixels.
        x: i32,
        /// Vertical screen position of the activation, in pixels.
        y: i32,
    },
    /// Tray icon secondary activated (typically a middle click).
    IconSecondaryActivated {
        /// Horizontal screen position of the activation, in pixels.
        x: i32,
        /// Vertical screen position of the activation, in pixels.
        y: i32,
    },
    /// A menu item was activated; `id` identifies the originating item.
    MenuItemActivated {
        /// The identifier of the activated item.
        id: MenuItemId,
    },
    /// Backend state changed; refresh the native presentation.
    Refresh,
}
