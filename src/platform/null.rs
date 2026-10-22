//! Fallback backend for platforms without a tray implementation. Always reports
//! itself unavailable so the host can degrade gracefully.

use gpui::{App, SharedString};

use crate::backend::TrayBackend;
use crate::icon::TrayIconData;
use crate::menu::TrayMenu;
use crate::LeftClick;

pub struct NullBackend;

impl TrayBackend for NullBackend {
    fn is_available(&self) -> bool {
        false
    }
    fn set_visible(&self, _visible: bool) {}
    fn set_icon(&self, _icon: Option<TrayIconData>) {}
    fn set_tooltip(&self, _tooltip: Option<SharedString>) {}
    fn set_menu(&self, _menu: TrayMenu) {}
    fn set_left_click_action(&self, _behavior: LeftClick) {}
}

pub fn create_backend(_cx: &mut App) -> anyhow::Result<NullBackend> {
    Ok(NullBackend)
}
