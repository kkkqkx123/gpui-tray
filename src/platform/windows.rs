//! Windows backend: `tray-icon` (Win32 `Shell_NotifyIcon` + native `HMENU`).
//!
//! The tray icon owns a hidden window on the thread that creates it, which is
//! the GPUI thread. `tray-icon` reports menu clicks through a global receiver;
//! a dedicated thread pumps it and forwards the clicks to the bridge task.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use gpui::{App, AsyncApp, SharedString};
use parking_lot::Mutex;
use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};

use crate::backend::TrayBackend;
use crate::event::TrayEvent;
use crate::icon::TrayIconData;
use crate::menu::{split_menu, MenuItemId, TrayMenu, TrayMenuItem};
use crate::AnyAction;
use crate::LeftClick;

pub struct WindowsBackend {
    tray: tray_icon::TrayIcon,
    shared: Arc<Mutex<WinShared>>,
}

struct WinShared {
    handlers: HashMap<MenuItemId, crate::menu::Handler>,
    left_click: LeftClick,
    available: bool,
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

/// Builds the native menu, giving every submenu an id of its own.
struct MenuBuilder {
    next_submenu: usize,
}

impl MenuBuilder {
    fn build(&mut self, items: &[TrayMenuItem]) -> Menu {
        let menu = Menu::new();
        self.fill(&menu, items);
        menu
    }

    fn fill(&mut self, sink: &dyn MenuSink, items: &[TrayMenuItem]) {
        for item in items {
            match item {
                TrayMenuItem::Separator => sink.push(&PredefinedMenuItem::separator()),
                TrayMenuItem::Submenu(sub) => {
                    let id = format!("__tray_submenu_{}", self.next_submenu);
                    self.next_submenu += 1;
                    let submenu = Submenu::with_id(id, &sub.label, true);
                    self.fill(&submenu, &sub.items);
                    sink.push(&submenu);
                }
                TrayMenuItem::Item(it) => {
                    let id = it.id.0.to_string();
                    if let Some(checked) = it.checked {
                        sink.push(&CheckMenuItem::with_id(
                            id,
                            &it.label,
                            it.enabled,
                            checked,
                            None,
                        ));
                    } else {
                        sink.push(&MenuItem::with_id(
                            id,
                            &it.label,
                            it.enabled,
                            None,
                        ));
                    }
                }
            }
        }
    }
}

pub fn create_backend(cx: &mut App) -> anyhow::Result<WindowsBackend> {
    let (tx, rx) = flume::unbounded::<TrayEvent>();

    // `tray-icon` reports menu clicks through a global receiver; pump it on a
    // dedicated thread and forward to the bridge over the (Send) flume channel.
    let tx_thread = tx.clone();
    thread::spawn(move || {
        let receiver = MenuEvent::receiver();
        while let Ok(event) = receiver.recv() {
            let id = MenuItemId(SharedString::from(event.id.0));
            let _ = tx_thread.send(TrayEvent::MenuItemActivated { id });
        }
    });

    let tray_menu = MenuBuilder { next_submenu: 0 }.build(&[]);
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .build()
        .map_err(|e| anyhow::anyhow!("failed to create tray icon: {e}"))?;

    let shared = Arc::new(Mutex::new(WinShared {
        handlers: HashMap::new(),
        left_click: LeftClick::Menu,
        available: true,
    }));

    let app = cx.to_async();
    let shared_task = Arc::clone(&shared);
    let _task = cx.spawn(move |_cx: &mut AsyncApp| {
        let app = app.clone();
        let shared = shared_task;
        let rx = rx;
        async move {
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
                    _ => {}
                }
            }
        }
    });

    Ok(WindowsBackend { tray, shared })
}

impl TrayBackend for WindowsBackend {
    fn is_available(&self) -> bool {
        self.shared.lock().available
    }

    fn set_visible(&self, visible: bool) {
        self.shared.lock().available = visible;
        let _ = self.tray.set_visible(visible);
    }

    fn set_icon(&self, icon: Option<TrayIconData>) {
        if let Some(icon) = icon {
            // `tray-icon` expects BGRA; convert from the tray's sRGBA. A production
            // build should also cache and reuse the `Icon` handle.
            let rgba = icon.rgba;
            let icon = Icon::from_rgba(rgba, icon.width, icon.height)
                .map_err(|e| anyhow::anyhow!("icon: {e}"));
            let _ = self.tray.set_icon(icon.ok());
        }
    }

    fn set_tooltip(&self, tooltip: Option<SharedString>) {
        if let Some(tip) = tooltip {
            let _ = self.tray.set_tooltip(Some(tip.to_string()));
        }
    }

    fn set_menu(&self, menu: TrayMenu) {
        let (menu, handlers) = split_menu(menu);
        let native = MenuBuilder {
            next_submenu: 0,
        }
        .build(&menu.items);
        self.tray.set_menu(Some(Box::new(native)));
        self.shared.lock().handlers = handlers;
    }

    fn set_left_click_action(&self, behavior: LeftClick) {
        self.shared.lock().left_click = behavior;
    }
}
