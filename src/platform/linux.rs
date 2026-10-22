//! Linux backend: `ksni` (StatusNotifierItem over DBus, zbus-based, no GTK).
//!
//! The tray runs on its own DBus service thread; menu/icon clicks are forwarded to
//! the GPUI main thread through a channel and dispatched as GPUI actions, mirroring
//! GPUI's own `init_app_menus` bridge.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use flume::Sender;
use gpui::{App, AsyncApp, Keystroke, Modifiers};
use ksni::{
    Category, Handle, Icon, OfflineReason, Status, ToolTip, Tray, TrayMethods,
    menu::{CheckmarkItem, Disposition, MenuItem, StandardItem, SubMenu},
};
use log::error;

use crate::backend::TrayBackend;
use crate::event::TrayEvent;
use crate::icon::TrayIconData;
use crate::LeftClick;
use crate::menu::{split_menu, MenuItemId, TrayMenu, TrayMenuItem, TraySubmenu};
use crate::AnyAction;

pub struct LinuxBackend {
    shared: Arc<Mutex<LinuxShared>>,
    tx: Sender<TrayEvent>,
}

struct LinuxShared {
    title: String,
    tooltip: Option<String>,
    icon_pixmaps: Vec<Icon>,
    menu: TrayMenu,
    handlers: HashMap<MenuItemId, crate::menu::Handler>,
    left_click: LeftClick,
    visible: bool,
    available: bool,
}

impl Default for LinuxShared {
    fn default() -> Self {
        Self {
            title: "gpui-tray".to_string(),
            tooltip: None,
            icon_pixmaps: Vec::new(),
            menu: TrayMenu::default(),
            handlers: HashMap::new(),
            left_click: LeftClick::Menu,
            visible: true,
            available: false,
        }
    }
}

/// The `ksni::Tray` implementation. Lives on the ksni service thread; all shared
/// state is behind the `Arc<Mutex<LinuxShared>>`.
struct KsniTray {
    shared: Arc<Mutex<LinuxShared>>,
    tx: Sender<TrayEvent>,
}

impl Tray for KsniTray {
    // Left click triggers `activate()` (we dispatch the configured action there);
    // the menu is opened by the shell on right click / context-menu signal.
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        self.shared.lock().title.clone()
    }

    fn title(&self) -> String {
        self.shared.lock().title.clone()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn status(&self) -> Status {
        if self.shared.lock().visible {
            Status::Active
        } else {
            Status::Passive
        }
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

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let g = self.shared.lock();
        build_items(&g.menu.items, &self.tx)
    }

    fn activate(&mut self, x: i32, y: i32) {
        let _ = self.tx.send(TrayEvent::IconActivated { x, y });
    }

    fn secondary_activate(&mut self, x: i32, y: i32) {
        let _ = self.tx.send(TrayEvent::IconSecondaryActivated { x, y });
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        self.shared.lock().available = false;
        // Returning `false` keeps the tray service running; we simply mark the tray
        // unavailable and the host degrades gracefully.
        false
    }

    fn watcher_online(&self) {
        self.shared.lock().available = true;
    }
}

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
    parts.push(k.key.to_uppercase());
    vec![parts]
}

fn build_items(items: &[TrayMenuItem], tx: &Sender<TrayEvent>) -> Vec<MenuItem<KsniTray>> {
    items
        .iter()
        .map(|item| match item {
            TrayMenuItem::Separator => MenuItem::Separator,
            TrayMenuItem::Submenu(sub) => MenuItem::SubMenu(build_submenu(sub, tx)),
            TrayMenuItem::Item(it) => {
                let id = it.id.clone();
                let tx = tx.clone();
                let shortcut = shortcut_of(it.accelerator.as_ref());
                if let Some(checked) = it.checked {
                    MenuItem::Checkmark(CheckmarkItem {
                        label: it.label.to_string(),
                        enabled: it.enabled,
                        visible: true,
                        checked,
                        icon_name: String::new(),
                        icon_data: Vec::new(),
                        shortcut,
                        disposition: Disposition::Normal,
                        activate: Box::new(move |_tray: &mut KsniTray| {
                            let _ = tx.send(TrayEvent::MenuItemActivated { id: id.clone() });
                        }),
                    })
                } else {
                    MenuItem::Standard(StandardItem {
                        label: it.label.to_string(),
                        enabled: it.enabled,
                        visible: true,
                        icon_name: String::new(),
                        icon_data: Vec::new(),
                        shortcut,
                        disposition: Disposition::Normal,
                        activate: Box::new(move |_tray: &mut KsniTray| {
                            let _ = tx.send(TrayEvent::MenuItemActivated { id: id.clone() });
                        }),
                    })
                }
            }
        })
        .collect()
}

fn build_submenu(sub: &TraySubmenu, tx: &Sender<TrayEvent>) -> SubMenu<KsniTray> {
    SubMenu {
        label: sub.label.to_string(),
        enabled: true,
        visible: true,
        icon_name: String::new(),
        icon_data: Vec::new(),
        shortcut: Vec::new(),
        disposition: Disposition::Normal,
        submenu: build_items(&sub.items, tx),
    }
}

pub fn create_backend(cx: &mut App) -> anyhow::Result<LinuxBackend> {
    let (tx, rx) = flume::unbounded::<TrayEvent>();
    let shared = Arc::new(Mutex::new(LinuxShared::default()));
    let handle_slot: Arc<Mutex<Option<Handle<KsniTray>>>> = Arc::new(Mutex::new(None));
    let tray = KsniTray {
        shared: Arc::clone(&shared),
        tx: tx.clone(),
    };

    let shared_task = Arc::clone(&shared);
    let app = cx.to_async();
    let _task = cx.spawn(move |_cx: &mut AsyncApp| {
        let app = app.clone();
        let shared = shared_task;
        let handle_slot = handle_slot;
        let rx = rx;
        async move {
            match tray.spawn().await {
                Ok(handle) => {
                    *handle_slot.lock() = Some(handle);
                    shared.lock().available = true;
                }
                Err(e) => {
                    error!("tray: failed to register StatusNotifierItem: {e}");
                    shared.lock().available = false;
                }
            }

            while let Ok(ev) = rx.recv_async().await {
                match ev {
                    TrayEvent::MenuItemActivated { id } => {
                        let action: Option<AnyAction> = shared
                            .lock()
                            .handlers
                            .get(&id)
                            .and_then(|h| h.action.clone());
                        if let Some(action) = action {
                            let _ = app.update(|cx| cx.dispatch_action(&*action));
                        } else {
                            let mut g = shared.lock();
                            if let Some(h) = g.handlers.get_mut(&id) {
                                if let Some(cb) = &mut h.on_activate {
                                    let _ = app.update(|cx| cb(cx));
                                }
                            }
                        }
                    }
                    TrayEvent::IconActivated { .. } => {
                        let left = shared.lock().left_click.clone();
                        if let LeftClick::Action(a) = left {
                            let _ = app.update(|cx| cx.dispatch_action(&*a));
                        }
                    }
                    TrayEvent::IconSecondaryActivated { .. } => {}
                    TrayEvent::Refresh => {
                        // Take the handle out of the slot before awaiting: the
                        // update future must not run while the lock is held.
                        let handle = handle_slot.lock().as_ref().cloned();
                        if let Some(h) = handle {
                            let _ = h.update(|_t: &mut KsniTray| {}).await;
                        }
                    }
                }
            }
        }
    });

    Ok(LinuxBackend {
        shared,
        tx,
    })
}

impl TrayBackend for LinuxBackend {
    fn is_available(&self) -> bool {
        self.shared.lock().available
    }

    fn set_visible(&self, visible: bool) {
        self.shared.lock().visible = visible;
        let _ = self.tx.send(TrayEvent::Refresh);
    }

    fn set_icon(&self, icon: Option<TrayIconData>) {
        let pix = icon
            .map(|d| {
                vec![Icon {
                    width: d.width as i32,
                    height: d.height as i32,
                    data: d.to_argb32(),
                }]
            })
            .unwrap_or_default();
        self.shared.lock().icon_pixmaps = pix;
        let _ = self.tx.send(TrayEvent::Refresh);
    }

    fn set_tooltip(&self, tooltip: Option<gpui::SharedString>) {
        self.shared.lock().tooltip = tooltip.map(|s| s.to_string());
        let _ = self.tx.send(TrayEvent::Refresh);
    }

    fn set_menu(&self, menu: TrayMenu) {
        let (menu, handlers) = split_menu(menu);
        let mut g = self.shared.lock();
        g.menu = menu;
        g.handlers = handlers;
        drop(g);
        let _ = self.tx.send(TrayEvent::Refresh);
    }

    fn set_left_click_action(&self, behavior: LeftClick) {
        self.shared.lock().left_click = behavior;
        let _ = self.tx.send(TrayEvent::Refresh);
    }
}
