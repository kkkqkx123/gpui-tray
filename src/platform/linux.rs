//! Linux backend: `ksni` (StatusNotifierItem / DBusMenu).
//!
//! The ksni service runs on its own DBus thread; watcher online/offline
//! callbacks arrive there too. A bridge task owned by the backend registers
//! the item, then forwards events to the runtime. The left-click behavior is
//! fixed at creation time because `ksni::Tray::MENU_ON_ACTIVATE` is an
//! associated constant — two concrete tray types carry the two variants.

#![cfg(target_os = "linux")]

use std::sync::Arc;

use gpui::{App, AsyncApp, Keystroke, Modifiers, SharedString};
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu};
use ksni::{Category, Icon, Status, ToolTip, TrayMethods};
use parking_lot::Mutex;

use crate::backend::TrayBackend;
use crate::error::{TrayAvailability, TrayError, TrayUnavailableReason};
use crate::event::{TrayButton, TrayButtonState, TrayEvent, TrayScrollOrientation};
use crate::icon::TrayIcon;
use crate::menu::{ResolvedEntry, ResolvedMenu};
use crate::options::TrayOptions;
use crate::runtime::{route_event, set_availability_and_notify, BackendSignal, RuntimeShared};

pub struct LinuxBackend {
    shared: Arc<Mutex<LinuxShared>>,
    tray_shared: Arc<RuntimeShared>,
    tx: flume::Sender<BackendSignal>,
    // Held so the bridge task lives as long as the backend; dropped when the
    // backend is dropped, which cancels the loop.
    _bridge: gpui::Task<()>,
}

/// State read by the ksni service thread on every DBus query.
struct LinuxShared {
    id: String,
    title: String,
    tooltip: Option<String>,
    visible: bool,
    attention: bool,
    icon_name: String,
    icon_pixmaps: Vec<Icon>,
    menu: ResolvedMenu,
}

impl Default for LinuxShared {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            tooltip: None,
            visible: true,
            attention: false,
            icon_name: String::new(),
            icon_pixmaps: Vec::new(),
            menu: ResolvedMenu::empty(),
        }
    }
}

/// The ksni service parts shared by both concrete tray types.
struct KsniBase {
    shared: Arc<Mutex<LinuxShared>>,
    tx: flume::Sender<BackendSignal>,
}

impl KsniBase {
    fn id(&self) -> String {
        self.shared.lock().id.clone()
    }

    fn title(&self) -> String {
        self.shared.lock().title.clone()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn status(&self) -> Status {
        let g = self.shared.lock();
        if g.attention {
            Status::NeedsAttention
        } else if g.visible {
            Status::Active
        } else {
            // `Passive` asks the shell to de-emphasize the icon; some shells
            // do not fully hide it. Use `Tray::remove` for a deterministic
            // removal instead.
            Status::Passive
        }
    }

    fn icon_name(&self) -> String {
        self.shared.lock().icon_name.clone()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.shared.lock().icon_pixmaps.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        let g = self.shared.lock();
        ToolTip {
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
            title: g.title.clone(),
            description: g.tooltip.clone().unwrap_or_default(),
        }
    }

    fn activate(&mut self, x: i32, y: i32) {
        let _ = self.tx.send(BackendSignal::Event(TrayEvent::Activated {
            button: TrayButton::Left,
            state: TrayButtonState::Up,
            position: crate::runtime::make_point(x as f64, y as f64, 1.0),
            rect: None,
        }));
    }

    fn secondary_activate(&mut self, x: i32, y: i32) {
        let _ = self.tx.send(BackendSignal::Event(TrayEvent::Activated {
            button: TrayButton::Middle,
            state: TrayButtonState::Up,
            position: crate::runtime::make_point(x as f64, y as f64, 1.0),
            rect: None,
        }));
    }

    fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
        let orientation = match orientation {
            ksni::Orientation::Horizontal => TrayScrollOrientation::Horizontal,
            ksni::Orientation::Vertical => TrayScrollOrientation::Vertical,
        };
        let _ = self.tx.send(BackendSignal::Event(TrayEvent::Scrolled {
            delta: delta as f64,
            orientation,
        }));
    }

    fn watcher_offline(
        &self,
        tray_shared: &Arc<RuntimeShared>,
        reason: ksni::OfflineReason,
    ) -> bool {
        match reason {
            ksni::OfflineReason::No => log::info!("tray: StatusNotifierWatcher went offline"),
            ksni::OfflineReason::Error(e) => log::warn!("tray: watcher error: {e}"),
            other => log::warn!("tray: watcher offline: {other:?}"),
        }
        set_availability_and_notify(
            tray_shared,
            &self.tx,
            TrayAvailability::Unavailable(TrayUnavailableReason::NoStatusNotifierHost),
        );
        // Keep the service running so the icon returns when a watcher appears.
        false
    }

    fn watcher_online(&self, tray_shared: &Arc<RuntimeShared>) {
        set_availability_and_notify(tray_shared, &self.tx, TrayAvailability::Available);
    }
}

/// Tray variant used when the left click opens the menu (`LeftClick::Menu`).
struct KsniMenuOnActivate {
    base: KsniBase,
    tray_shared: Arc<RuntimeShared>,
}

/// Tray variant used when the left click dispatches an action or does nothing
/// (`LeftClick::Action` / `LeftClick::None`).
struct KsniActionOnActivate {
    base: KsniBase,
    tray_shared: Arc<RuntimeShared>,
}

macro_rules! impl_ksni_tray {
    ($ty:ident, $menu_on_activate:expr) => {
        impl ksni::Tray for $ty {
            const MENU_ON_ACTIVATE: bool = $menu_on_activate;

            fn id(&self) -> String {
                self.base.id()
            }
            fn title(&self) -> String {
                self.base.title()
            }
            fn category(&self) -> Category {
                self.base.category()
            }
            fn status(&self) -> Status {
                self.base.status()
            }
            fn icon_name(&self) -> String {
                self.base.icon_name()
            }
            fn icon_pixmap(&self) -> Vec<Icon> {
                self.base.icon_pixmap()
            }
            fn tool_tip(&self) -> ToolTip {
                self.base.tool_tip()
            }
            fn menu(&self) -> Vec<MenuItem<Self>> {
                // The menu graph is identical for both variants; the per-type
                // `MenuItem<Self>` is rebuilt from the shared snapshot every
                // time the shell asks for it.
                build_entries::<Self>(&self.base.shared.lock().menu.entries, &self.base.tx)
            }
            fn activate(&mut self, x: i32, y: i32) {
                self.base.activate(x, y);
            }
            fn secondary_activate(&mut self, x: i32, y: i32) {
                self.base.secondary_activate(x, y);
            }
            fn scroll(&mut self, delta: i32, orientation: ksni::Orientation) {
                self.base.scroll(delta, orientation);
            }
            fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
                self.base.watcher_offline(&self.tray_shared, reason)
            }
            fn watcher_online(&self) {
                self.base.watcher_online(&self.tray_shared);
            }
        }
    };
}

impl_ksni_tray!(KsniMenuOnActivate, true);
impl_ksni_tray!(KsniActionOnActivate, false);

fn shortcut_of(k: Option<&Keystroke>) -> Vec<Vec<String>> {
    let Some(k) = k else {
        return Vec::new();
    };
    let modifiers: Modifiers = k.modifiers;
    let mut parts: Vec<String> = Vec::new();
    if modifiers.control {
        parts.push("Control".to_string());
    }
    if modifiers.alt {
        parts.push("Alt".to_string());
    }
    if modifiers.shift {
        parts.push("Shift".to_string());
    }
    if modifiers.platform {
        parts.push("Super".to_string());
    }
    parts.push(k.key.clone());
    vec![parts]
}

fn build_entries<T: ksni::Tray>(
    entries: &[ResolvedEntry],
    tx: &flume::Sender<BackendSignal>,
) -> Vec<MenuItem<T>> {
    entries
        .iter()
        .map(|entry| match entry {
            ResolvedEntry::Separator => MenuItem::Separator,
            ResolvedEntry::Item(item) => {
                let id = item.id().clone();
                let tx = tx.clone();
                let activate = Box::new(move |_tray: &mut T| {
                    let _ = tx.send(BackendSignal::Event(TrayEvent::MenuItemActivated {
                        id: id.clone(),
                    }));
                });
                match item.checked() {
                    Some(checked) => {
                        let mut ck = CheckmarkItem::default();
                        ck.label = item.label().to_string();
                        ck.enabled = item.is_enabled();
                        ck.checked = checked;
                        ck.shortcut = shortcut_of(item.accelerator());
                        ck.activate = activate;
                        MenuItem::Checkmark(ck)
                    }
                    None => {
                        let mut std_item = StandardItem::default();
                        std_item.label = item.label().to_string();
                        std_item.enabled = item.is_enabled();
                        std_item.shortcut = shortcut_of(item.accelerator());
                        std_item.activate = activate;
                        MenuItem::Standard(std_item)
                    }
                }
            }
            ResolvedEntry::Submenu(sub) => MenuItem::SubMenu(SubMenu {
                label: sub.label().to_string(),
                submenu: build_entries::<T>(sub.entries(), tx),
                ..Default::default()
            }),
        })
        .collect()
}

pub fn create_backend(
    cx: &mut App,
    options: &TrayOptions,
    tray_shared: Arc<RuntimeShared>,
) -> Result<LinuxBackend, TrayError> {
    let (tx, rx) = flume::unbounded::<BackendSignal>();
    let shared = Arc::new(Mutex::new(LinuxShared {
        // The SNI identity is the tray id, not the title, so changing the
        // title does not change the item's identity on the bus.
        id: options.id().as_str().to_string(),
        title: options
            .title()
            .map(|t| t.to_string())
            .unwrap_or_else(|| options.id().as_str().to_string()),
        tooltip: options.tooltip().map(|t| t.to_string()),
        ..Default::default()
    }));

    let bridge = match &options.left_click {
        crate::LeftClick::Menu => {
            let base = KsniBase {
                shared: Arc::clone(&shared),
                tx: tx.clone(),
            };
            let tray = KsniMenuOnActivate {
                base,
                tray_shared: Arc::clone(&tray_shared),
            };
            run_bridge(cx, tray, &tray_shared, &tx, &rx)
        }
        _ => {
            let base = KsniBase {
                shared: Arc::clone(&shared),
                tx: tx.clone(),
            };
            let tray = KsniActionOnActivate {
                base,
                tray_shared: Arc::clone(&tray_shared),
            };
            run_bridge(cx, tray, &tray_shared, &tx, &rx)
        }
    };

    Ok(LinuxBackend {
        tray_shared,
        shared,
        tx,
        _bridge: bridge,
    })
}

fn run_bridge<T: ksni::Tray>(
    cx: &mut App,
    tray: T,
    tray_shared: &Arc<RuntimeShared>,
    tx: &flume::Sender<BackendSignal>,
    rx: &flume::Receiver<BackendSignal>,
) -> gpui::Task<()> {
    let tray_shared_loop = Arc::clone(tray_shared);
    let tx_loop = tx.clone();
    let app = cx.to_async();
    let rx = rx.clone();
    cx.spawn(move |_cx: &mut AsyncApp| {
        let app = app.clone();
        let tray_shared = tray_shared_loop;
        let tx = tx_loop;
        let rx = rx;
        async move {
            match tray.spawn().await {
                Ok(handle) => {
                    set_availability_and_notify(&tray_shared, &tx, TrayAvailability::Available);
                    loop {
                        if tray_shared.is_closed() {
                            break;
                        }
                        match rx.recv_async().await {
                            Ok(BackendSignal::Event(event)) => {
                                route_event(&tray_shared, &app, event);
                            }
                            Ok(BackendSignal::Refresh) => {
                                let _ = handle.update(|_| {}).await;
                            }
                            Ok(BackendSignal::Shutdown) => {
                                handle.shutdown();
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                }
                Err(e) => {
                    log::error!("tray: failed to register StatusNotifierItem: {e}");
                    set_availability_and_notify(
                        &tray_shared,
                        &tx,
                        TrayAvailability::Unavailable(TrayUnavailableReason::NoStatusNotifierHost),
                    );
                }
            }
        }
    })
}

impl TrayBackend for LinuxBackend {
    fn is_available(&self) -> bool {
        self.tray_shared.availability().is_available()
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        self.shared.lock().visible = visible;
        let _ = self.tx.send(BackendSignal::Refresh);
        Ok(())
    }

    fn set_icon(&self, icon: Option<TrayIcon>) -> Result<(), TrayError> {
        let mut g = self.shared.lock();
        match icon {
            Some(TrayIcon::Named(name)) => {
                g.icon_name = name.as_str().to_string();
                g.icon_pixmaps.clear();
            }
            Some(TrayIcon::Bitmap(bitmap)) => {
                g.icon_name.clear();
                g.icon_pixmaps = bitmap
                    .images()
                    .iter()
                    .map(|img| Icon {
                        width: img.pixel_width() as i32,
                        height: img.pixel_height() as i32,
                        data: img.to_argb32(),
                    })
                    .collect();
            }
            None => {
                g.icon_name.clear();
                g.icon_pixmaps.clear();
            }
        }
        drop(g);
        let _ = self.tx.send(BackendSignal::Refresh);
        Ok(())
    }

    fn set_tooltip(&self, tooltip: Option<SharedString>) -> Result<(), TrayError> {
        self.shared.lock().tooltip = tooltip.map(|s| s.to_string());
        let _ = self.tx.send(BackendSignal::Refresh);
        Ok(())
    }

    fn set_title(&self, title: Option<SharedString>) -> Result<(), TrayError> {
        let mut g = self.shared.lock();
        g.title = title
            .map(|t| t.to_string())
            .unwrap_or_else(|| g.id.clone());
        drop(g);
        let _ = self.tx.send(BackendSignal::Refresh);
        Ok(())
    }

    fn set_menu(&self, menu: &ResolvedMenu) -> Result<(), TrayError> {
        self.shared.lock().menu = menu.clone();
        let _ = self.tx.send(BackendSignal::Refresh);
        Ok(())
    }

    fn show_menu(&self) -> Result<(), TrayError> {
        // The shell owns the DBusMenu; SNI has no programmatic open signal.
        Err(TrayError::UnsupportedPlatform)
    }

    fn request_attention(&self, attention: bool) -> Result<(), TrayError> {
        self.shared.lock().attention = attention;
        let _ = self.tx.send(BackendSignal::Refresh);
        Ok(())
    }

    fn shutdown(&self) {
        let _ = self.tx.send(BackendSignal::Shutdown);
    }
}
