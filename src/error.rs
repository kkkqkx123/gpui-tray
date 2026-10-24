//! Availability and error types shared by the tray API.

use gpui::SharedString;
use std::fmt;

/// Why a tray is not (yet) usable on the current platform or session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayUnavailableReason {
    /// The platform has no tray implementation at all.
    UnsupportedPlatform,
    /// Linux: no StatusNotifierWatcher is present (e.g. GNOME without the
    /// AppIndicator extension), or it went away.
    NoStatusNotifierHost,
    /// The backend reported a platform-specific failure.
    Native(SharedString),
}

impl fmt::Display for TrayUnavailableReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => write!(f, "unsupported platform"),
            Self::NoStatusNotifierHost => write!(f, "no StatusNotifier host (install an AppIndicator extension)"),
            Self::Native(reason) => write!(f, "native error: {reason}"),
        }
    }
}

/// Whether the tray is currently usable.
///
/// Availability is independent of visibility: hiding the icon never changes
/// availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAvailability {
    /// Linux only: the StatusNotifierItem registration is still in flight.
    Registering,
    /// The tray is registered and usable.
    Available,
    /// The tray cannot be used on this platform or session.
    Unavailable(TrayUnavailableReason),
}

impl TrayAvailability {
    /// `true` only in the [`TrayAvailability::Available`] state.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// An error returned by tray operations.
///
/// All setters report failures instead of silently succeeding, so a host can
/// decide whether to surface them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayError {
    /// The tray was removed and no longer accepts operations.
    Closed,
    /// The platform does not support the requested capability.
    UnsupportedPlatform,
    /// The tray is not usable in the current session.
    Unavailable(TrayUnavailableReason),
    /// The operation must run on the main thread.
    WrongThread,
    /// The icon bitmap is malformed (size/length mismatch).
    InvalidIcon(SharedString),
    /// The menu is malformed (e.g. duplicate item ids).
    InvalidMenu(SharedString),
    /// The native backend reported an error.
    Native(SharedString),
}

impl fmt::Display for TrayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "tray removed"),
            Self::UnsupportedPlatform => write!(f, "unsupported platform"),
            Self::Unavailable(reason) => write!(f, "tray unavailable: {reason}"),
            Self::WrongThread => write!(f, "operation must run on the main thread"),
            Self::InvalidIcon(details) => write!(f, "invalid icon: {details}"),
            Self::InvalidMenu(details) => write!(f, "invalid menu: {details}"),
            Self::Native(details) => write!(f, "native error: {details}"),
        }
    }
}

impl std::error::Error for TrayError {}
