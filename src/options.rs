//! Creation-time options for a tray.

use gpui::SharedString;

use crate::icon::TrayIcon;
use crate::LeftClick;

/// A stable identifier for a tray instance, provided by the application
/// (reverse-DNS style is recommended, e.g. `com.example.app.tray`).
///
/// Used as the StatusNotifierItem id on Linux, the `TrayIconId` on Windows and
/// the namespace of native menu identifiers on every platform.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrayId {
    id: SharedString,
}

impl TrayId {
    /// Create a tray id.
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self { id: id.into() }
    }

    /// The identifier as a string.
    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl From<&str> for TrayId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// Creation-time options for [`crate::Tray`].
///
/// Built through [`TrayOptions::builder`]. Items that cannot safely change at
/// runtime — most notably the left-click behavior, because the Linux backend
/// fixes it when the StatusNotifierItem is registered — are fixed here.
#[derive(Clone)]
pub struct TrayOptions {
    pub(crate) id: TrayId,
    pub(crate) left_click: LeftClick,
    pub(crate) title: Option<SharedString>,
    pub(crate) tooltip: Option<SharedString>,
    pub(crate) icon: Option<TrayIcon>,
}

impl TrayOptions {
    /// Start building options for a tray with the given id.
    pub fn builder(id: impl Into<TrayId>) -> TrayOptionsBuilder {
        TrayOptionsBuilder {
            id: id.into(),
            left_click: LeftClick::Menu,
            title: None,
            tooltip: None,
            icon: None,
        }
    }

    /// The tray id.
    pub fn id(&self) -> &TrayId {
        &self.id
    }

    /// The configured left-click behavior.
    pub fn left_click(&self) -> &LeftClick {
        &self.left_click
    }

    /// The configured title, if any.
    pub fn title(&self) -> Option<&SharedString> {
        self.title.as_ref()
    }

    /// The configured tooltip, if any.
    pub fn tooltip(&self) -> Option<&SharedString> {
        self.tooltip.as_ref()
    }

    /// The configured icon, if any.
    pub fn icon(&self) -> Option<&TrayIcon> {
        self.icon.as_ref()
    }
}

/// Builder for [`TrayOptions`].
pub struct TrayOptionsBuilder {
    id: TrayId,
    left_click: LeftClick,
    title: Option<SharedString>,
    tooltip: Option<SharedString>,
    icon: Option<TrayIcon>,
}

impl TrayOptionsBuilder {
    /// Set what a left click (activation) does. Defaults to
    /// [`LeftClick::Menu`].
    pub fn with_left_click(mut self, behavior: LeftClick) -> Self {
        self.left_click = behavior;
        self
    }

    /// Set the initial title (Linux/macOS).
    pub fn with_title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the initial tooltip.
    pub fn with_tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Set the initial icon.
    pub fn with_icon(mut self, icon: TrayIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Finish building.
    pub fn build(self) -> TrayOptions {
        TrayOptions {
            id: self.id,
            left_click: self.left_click,
            title: self.title,
            tooltip: self.tooltip,
            icon: self.icon,
        }
    }
}
