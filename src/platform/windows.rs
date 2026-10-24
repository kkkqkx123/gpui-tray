//! Windows backend: `tray-icon` (Win32 `Shell_NotifyIcon` + native `HMENU`).
//!
//! `tray-icon` reports menu clicks and tray events through process-global
//! handlers. One handler pair is installed for the whole process and routes
//! into the per-tray runtime by `TrayId`, replacing the previous
//! "one receiver thread per tray" model that raced on the global channel.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use gpui::{App, AsyncApp, Keystroke, Modifiers, SharedString};
use parking_lot::Mutex;
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers as MudaModifiers};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{MouseButton, MouseButtonState, Rect, TrayIcon as NativeTrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::backend::TrayBackend;
use crate::error::TrayError;
use crate::event::{TrayButton, TrayButtonState, TrayEvent, TrayRect};
use crate::icon::TrayIcon;
use crate::menu::{ResolvedEntry, ResolvedMenu, TrayItemId};
use crate::options::{TrayId, TrayOptions};
use crate::runtime::{route_event, make_rect, BackendSignal, RuntimeShared};

pub struct WindowsBackend {
    tray: NativeTrayIcon,
    tray_id: TrayId,
    // Held so the bridge task lives as long as the backend; dropped when the
    // backend is dropped, which cancels the loop.
    _bridge: gpui::Task<()>,
    #[allow(dead_code)]
    shared: Arc<RuntimeShared>,
}

/// Registry of the runtimes that receive global menu/tray events, keyed by
/// `TrayId`. Weak ownership: a runtime must unregister on shutdown.
static ROUTER: OnceLock<Mutex<HashMap<String, Arc<RuntimeShared>>>> = OnceLock::new();
static PUMPS_INSTALLED: OnceLock<()> = OnceLock::new();

fn router() -> &'static Mutex<HashMap<String, Arc<RuntimeShared>>> {
    ROUTER.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sender_for(tray_id: &str) -> Option<(Arc<RuntimeShared>, flume::Sender<BackendSignal>)> {
    router().lock().get(tray_id).map(|shared| {
        // The bridge task holds its own rx; events are delivered by sending
        // through the per-runtime channel stored on the shared state.
        (shared.clone(), shared.event_sender())
    })
}

/// Install the process-global event handlers once. They only enqueue events;
/// touching GPUI happens on the main thread in the bridge task.
fn ensure_pumps() {
    PUMPS_INSTALLED.get_or_init(|| {
        TrayIconEvent::set_event_handler(Some(|event| route_icon_event(event)));
        MenuEvent::set_event_handler(Some(|event| route_menu_event(event)));
    });
}

/// Menu ids are prefixed with the tray id so the global handler can route the
/// event to the owning runtime.
fn native_item_id(tray_id: &str, item_id: &TrayItemId) -> String {
    format!("{tray_id}:{}", item_id.as_str())
}

fn route_menu_event(event: MenuEvent) {
    let raw = event.id.0.clone();
    let Some((tray_id, item_id)) = raw.split_once(':') else {
        return;
    };
    if let Some((shared, _)) = sender_for(tray_id) {
        shared.send_backend_signal(BackendSignal::Event(TrayEvent::MenuItemActivated {
            id: TrayItemId::new(item_id.to_string()),
        }));
    }
}

fn route_icon_event(event: TrayIconEvent) {
    let (tray_id, converted) = match convert_icon_event(event) {
        Some(converted) => converted,
        None => return,
    };
    if let Some((shared, _)) = sender_for(&tray_id) {
        shared.send_backend_signal(BackendSignal::Event(converted));
    }
}

fn convert_icon_event(event: TrayIconEvent) -> Option<(String, TrayEvent)> {
    match event {
        TrayIconEvent::Click {
            id,
            button,
            button_state,
            position,
            rect,
            ..
        } => Some((
            id.0.clone(),
            TrayEvent::Activated {
                button: convert_button(button)?,
                state: convert_state(button_state),
                position: convert_position(position),
                rect: convert_rect(rect),
            },
        )),
        TrayIconEvent::DoubleClick {
            id, position, rect, ..
        } => Some((
            id.0.clone(),
            TrayEvent::DoubleClicked {
                button: TrayButton::Left,
                position: convert_position(position),
                rect: convert_rect(rect),
            },
        )),
        _ => None,
    }
}

fn convert_button(button: MouseButton) -> Option<TrayButton> {
    match button {
        MouseButton::Left => Some(TrayButton::Left),
        MouseButton::Right => Some(TrayButton::Right),
        MouseButton::Middle => Some(TrayButton::Middle),
        _ => None,
    }
}

fn convert_state(state: MouseButtonState) -> TrayButtonState {
    match state {
        MouseButtonState::Up => TrayButtonState::Up,
        MouseButtonState::Down => TrayButtonState::Down,
    }
}

fn convert_position(position: tray_icon::dpi::PhysicalPosition<f64>) -> crate::event::TrayPoint {
    crate::runtime::make_point(position.x, position.y, 1.0)
}

fn convert_rect(rect: Rect) -> Option<TrayRect> {
    Some(make_rect(
        rect.position.x,
        rect.position.y,
        rect.size.width as f64,
        rect.size.height as f64,
        1.0,
    ))
}

/// A menu that items can be appended to.
///
/// `muda` gives `Menu` and `Submenu` their own `append` methods instead of a
/// shared trait, so the two are unified here.
trait MenuSink {
    fn push(&self, item: &dyn IsMenuItem);
}

impl MenuSink for Menu {
    fn push(&self, item: &dyn IsMenuItem) {
        let _ = self.append(item);
    }
}

impl MenuSink for Submenu {
    fn push(&self, item: &dyn IsMenuItem) {
        let _ = self.append(item);
    }
}

fn build_menu(tray_id: &str, menu: &ResolvedMenu) -> Menu {
    let native = Menu::new();
    fill_menu(&native, tray_id, &menu.entries);
    native
}

fn fill_menu(sink: &dyn MenuSink, tray_id: &str, entries: &[ResolvedEntry]) {
    for entry in entries {
        match entry {
            ResolvedEntry::Separator => sink.push(&PredefinedMenuItem::separator()),
            ResolvedEntry::Submenu(sub) => {
                // Submenus do not carry their own stable id; the item ids
                // inside them already route through the tray prefix.
                let submenu = Submenu::new(sub.label().as_ref(), true);
                fill_menu(&submenu, tray_id, sub.entries());
                sink.push(&submenu);
            }
            ResolvedEntry::Item(item) => {
                let id = native_item_id(tray_id, item.id());
                let accelerator = item.accelerator().and_then(accelerator_of);
                match item.checked() {
                    Some(checked) => sink.push(&CheckMenuItem::with_id(
                        id,
                        item.label().as_ref(),
                        item.is_enabled(),
                        checked,
                        accelerator,
                    )),
                    None => sink.push(&MenuItem::with_id(
                        id,
                        item.label().as_ref(),
                        item.is_enabled(),
                        accelerator,
                    )),
                }
            }
        }
    }
}

/// Map a GPUI `Keystroke` to a muda `Accelerator` for display in the menu.
fn accelerator_of(k: &Keystroke) -> Option<Accelerator> {
    let modifiers: Modifiers = k.modifiers;
    let mut mods = MudaModifiers::empty();
    if modifiers.control {
        mods |= MudaModifiers::CONTROL;
    }
    if modifiers.alt {
        mods |= MudaModifiers::ALT;
    }
    if modifiers.shift {
        mods |= MudaModifiers::SHIFT;
    }
    if modifiers.platform {
        mods |= MudaModifiers::SUPER;
    }
    let code = muda_key_code(&k.key)?;
    Some(Accelerator::new(Some(mods), code))
}

fn muda_key_code(key: &str) -> Option<Code> {
    match key {
        "a" => Some(Code::KeyA),
        "b" => Some(Code::KeyB),
        "c" => Some(Code::KeyC),
        "d" => Some(Code::KeyD),
        "e" => Some(Code::KeyE),
        "f" => Some(Code::KeyF),
        "g" => Some(Code::KeyG),
        "h" => Some(Code::KeyH),
        "i" => Some(Code::KeyI),
        "j" => Some(Code::KeyJ),
        "k" => Some(Code::KeyK),
        "l" => Some(Code::KeyL),
        "m" => Some(Code::KeyM),
        "n" => Some(Code::KeyN),
        "o" => Some(Code::KeyO),
        "p" => Some(Code::KeyP),
        "q" => Some(Code::KeyQ),
        "r" => Some(Code::KeyR),
        "s" => Some(Code::KeyS),
        "t" => Some(Code::KeyT),
        "u" => Some(Code::KeyU),
        "v" => Some(Code::KeyV),
        "w" => Some(Code::KeyW),
        "x" => Some(Code::KeyX),
        "y" => Some(Code::KeyY),
        "z" => Some(Code::KeyZ),
        other => {
            if let Some(digit) = other.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                // F1-F12
                Some(match digit {
                    1 => Code::F1,
                    2 => Code::F2,
                    3 => Code::F3,
                    4 => Code::F4,
                    5 => Code::F5,
                    6 => Code::F6,
                    7 => Code::F7,
                    8 => Code::F8,
                    9 => Code::F9,
                    10 => Code::F10,
                    11 => Code::F11,
                    12 => Code::F12,
                    _ => return None,
                })
            } else {
                None
            }
        }
    }
}

pub fn create_backend(
    cx: &mut App,
    options: &TrayOptions,
    shared: Arc<RuntimeShared>,
) -> Result<WindowsBackend, TrayError> {
    ensure_pumps();

    let (tx, rx) = flume::unbounded::<BackendSignal>();
    shared.set_event_sender(tx.clone());

    // Register in the global router before any native event can fire.
    router().lock().insert(options.id().as_str().to_string(), Arc::clone(&shared));

    let menu_on_left_click = matches!(options.left_click(), crate::LeftClick::Menu);
    let tray_menu = build_menu(options.id().as_str(), &ResolvedMenu::empty());
    let mut builder = TrayIconBuilder::new()
        .with_id(options.id().as_str())
        .with_menu(Box::new(tray_menu))
        .with_menu_on_left_click(menu_on_left_click);
    if let Some(tooltip) = options.tooltip() {
        builder = builder.with_tooltip(tooltip.to_string());
    }
    if let Some(TrayIcon::Bitmap(bitmap)) = options.icon() {
        if let Some(image) = bitmap.images().first() {
            let icon = tray_icon::Icon::from_rgba(
                image.rgba().to_vec(),
                image.pixel_width(),
                image.pixel_height(),
            )
            .map_err(|e| crate::runtime::native_error(e))?;
            builder = builder.with_icon(icon).with_icon_as_template(bitmap.is_templated());
        }
    }
    let tray = builder
        .build()
        .map_err(|e| crate::runtime::native_error(e))?;

    let app = cx.to_async();
    let shared_task = Arc::clone(&shared);
    let bridge = cx.spawn(move |_cx: &mut AsyncApp| {
        let app = app.clone();
        let shared = shared_task;
        let rx = rx;
        async move {
            shared.set_availability(crate::error::TrayAvailability::Available);
            while let Ok(signal) = rx.recv_async().await {
                if shared.is_closed() {
                    break;
                }
                match signal {
                    BackendSignal::Event(event) => {
                        if let TrayEvent::Activated { rect, .. } = &event {
                            shared.store_rect(*rect);
                        }
                        route_event(&shared, &app, event);
                    }
                    // Windows applies state changes synchronously; no pump is
                    // needed. A shutdown signal ends the loop.
                    BackendSignal::Refresh => {}
                    BackendSignal::Shutdown => break,
                }
            }
        }
    });

    Ok(WindowsBackend {
        tray,
        tray_id: options.id().clone(),
        _bridge: bridge,
        shared,
    })
}

impl TrayBackend for WindowsBackend {
    fn is_available(&self) -> bool {
        self.shared.availability().is_available()
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        self.tray
            .set_visible(visible)
            .map_err(|e| crate::runtime::native_error(e))
    }

    fn set_icon(&self, icon: Option<TrayIcon>) -> Result<(), TrayError> {
        // `tray-icon`'s `Icon::from_rgba` receives RGBA; an earlier comment
        // claiming BGRA was wrong.
        let converted = match icon {
            Some(TrayIcon::Bitmap(bitmap)) => {
                let Some(image) = bitmap.images().first() else {
                    return Err(TrayError::InvalidIcon("bitmap has no images".into()));
                };
                Some(
                    tray_icon::Icon::from_rgba(
                        image.rgba().to_vec(),
                        image.pixel_width(),
                        image.pixel_height(),
                    )
                    .map_err(|e| crate::runtime::native_error(e))?,
                )
            }
            Some(TrayIcon::Named(_)) => {
                return Err(TrayError::UnsupportedPlatform);
            }
            None => None,
        };
        self.tray
            .set_icon(converted)
            .map_err(|e| crate::runtime::native_error(e))
    }

    fn set_tooltip(&self, tooltip: Option<SharedString>) -> Result<(), TrayError> {
        self.tray
            .set_tooltip(tooltip.map(|t| t.to_string()))
            .map_err(|e| crate::runtime::native_error(e))
    }

    fn set_title(&self, title: Option<SharedString>) -> Result<(), TrayError> {
        self.tray.set_title(title.map(|t| t.to_string()));
        Ok(())
    }

    fn set_menu(&self, menu: &ResolvedMenu) -> Result<(), TrayError> {
        let native = build_menu(self.tray_id.as_str(), menu);
        self.tray.set_menu(Some(Box::new(native)));
        Ok(())
    }

    fn show_menu(&self) -> Result<(), TrayError> {
        // tray-icon 0.20 offers no programmatic menu popup on Windows.
        Err(TrayError::UnsupportedPlatform)
    }

    fn request_attention(&self, _attention: bool) -> Result<(), TrayError> {
        Err(TrayError::UnsupportedPlatform)
    }

    fn shutdown(&self) {
        let _ = self.tray.set_visible(false);
        router().lock().remove(self.tray_id.as_str());
        self.shared.clear_handlers();
    }
}
