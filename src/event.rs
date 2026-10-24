//! Events produced by the tray and delivered to the application.

use crate::error::TrayAvailability;
use crate::menu::TrayItemId;

/// Which mouse button produced an activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayButton {
    /// The left (primary) button.
    Left,
    /// The right (secondary) button.
    Right,
    /// The middle button.
    Middle,
}

/// Whether a button was pressed or released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayButtonState {
    /// The button was released.
    Up,
    /// The button was pressed.
    Down,
}

/// Scroll orientation reported by a scroll event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayScrollOrientation {
    /// Vertical scrolling.
    Vertical,
    /// Horizontal scrolling.
    Horizontal,
}

/// A position in screen coordinates.
///
/// Values are physical pixels plus the scale factor of the monitor the tray
/// icon is on; the factor is `1.0` when the platform does not report one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrayPoint {
    x: f64,
    y: f64,
    scale_factor: f32,
}

impl TrayPoint {
    pub(crate) fn new(x: f64, y: f64, scale_factor: f32) -> Self {
        Self {
            x,
            y,
            scale_factor,
        }
    }

    /// Horizontal position, in physical pixels.
    pub fn x(&self) -> f64 {
        self.x
    }

    /// Vertical position, in physical pixels.
    pub fn y(&self) -> f64 {
        self.y
    }

    /// The monitor scale factor; `1.0` when unknown.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }
}

/// The size of a rectangle, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TraySize {
    width: f64,
    height: f64,
}

impl TraySize {
    // Used by the Windows backend; dead on other targets.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }

    /// Width, in physical pixels.
    pub fn width(&self) -> f64 {
        self.width
    }

    /// Height, in physical pixels.
    pub fn height(&self) -> f64 {
        self.height
    }
}

/// The bounding rectangle of the tray icon on screen.
///
/// Linux does not report the icon rectangle; events carry `None` there and
/// [`crate::Tray::rect`] also returns `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrayRect {
    position: TrayPoint,
    size: TraySize,
}

impl TrayRect {
    // Used by the Windows backend; dead on other targets.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn new(position: TrayPoint, size: TraySize) -> Self {
        Self { position, size }
    }

    /// Top-left corner of the icon, in physical pixels.
    pub fn position(&self) -> &TrayPoint {
        &self.position
    }

    /// Size of the icon, in physical pixels.
    pub fn size(&self) -> &TraySize {
        &self.size
    }
}

/// An event originating from the native tray.
#[derive(Debug, Clone)]
pub enum TrayEvent {
    /// The availability state changed (registration finished, watcher went
    /// away, …).
    AvailabilityChanged {
        /// The new availability state.
        availability: TrayAvailability,
    },
    /// The tray icon was activated with a button.
    Activated {
        /// Which button was used.
        button: TrayButton,
        /// Whether the button was pressed or released.
        state: TrayButtonState,
        /// Cursor position, in physical pixels.
        position: TrayPoint,
        /// Icon rectangle, when the platform reports one.
        rect: Option<TrayRect>,
    },
    /// The tray icon was double clicked.
    ///
    /// Not reported on every platform; macOS only reports double clicks when
    /// no menu is attached.
    DoubleClicked {
        /// Which button was used.
        button: TrayButton,
        /// Cursor position, in physical pixels.
        position: TrayPoint,
        /// Icon rectangle, when the platform reports one.
        rect: Option<TrayRect>,
    },
    /// The user scrolled on the icon. Linux only.
    Scrolled {
        /// Scroll delta; sign follows the platform convention.
        delta: f64,
        /// Scroll axis.
        orientation: TrayScrollOrientation,
    },
    /// The tray menu is about to open. Only sent on platforms with an
    /// open-before hook.
    MenuWillOpen,
    /// The tray menu closed.
    MenuDidClose,
    /// A menu item was activated; `id` identifies the originating item.
    MenuItemActivated {
        /// The identifier of the activated item.
        id: TrayItemId,
    },
}
