//! Fallback backend for platforms without a tray implementation. Always reports
//! itself unavailable so the host can degrade gracefully.

use gpui::{App, SharedString};

use crate::backend::TrayBackend;
use crate::error::TrayError;
use crate::icon::TrayIcon;
use crate::menu::ResolvedMenu;
use crate::options::TrayOptions;
use crate::runtime::RuntimeShared;
use std::sync::Arc;

pub struct NullBackend {
    _shared: Arc<RuntimeShared>,
}

impl TrayBackend for NullBackend {
    fn is_available(&self) -> bool {
        false
    }
    fn set_visible(&self, _visible: bool) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(
            crate::error::TrayUnavailableReason::UnsupportedPlatform,
        ))
    }
    fn set_icon(&self, _icon: Option<TrayIcon>) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(
            crate::error::TrayUnavailableReason::UnsupportedPlatform,
        ))
    }
    fn set_tooltip(&self, _tooltip: Option<SharedString>) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(
            crate::error::TrayUnavailableReason::UnsupportedPlatform,
        ))
    }
    fn set_title(&self, _title: Option<SharedString>) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(
            crate::error::TrayUnavailableReason::UnsupportedPlatform,
        ))
    }
    fn set_menu(&self, _menu: &ResolvedMenu) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(
            crate::error::TrayUnavailableReason::UnsupportedPlatform,
        ))
    }
    fn show_menu(&self) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(
            crate::error::TrayUnavailableReason::UnsupportedPlatform,
        ))
    }
    fn request_attention(&self, _attention: bool) -> Result<(), TrayError> {
        Err(TrayError::Unavailable(
            crate::error::TrayUnavailableReason::UnsupportedPlatform,
        ))
    }
    fn shutdown(&self) {}
}

pub fn create_backend(
    _cx: &mut App,
    _options: &TrayOptions,
    shared: Arc<RuntimeShared>,
) -> Result<NullBackend, TrayError> {
    // Register the platform as unavailable so the runtime state matches.
    shared.set_availability(crate::error::TrayAvailability::Unavailable(
        crate::error::TrayUnavailableReason::UnsupportedPlatform,
    ));
    Ok(NullBackend { _shared: shared })
}
