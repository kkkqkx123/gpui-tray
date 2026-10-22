//! Windows backend: `tray-icon` (Win32 `Shell_NotifyIcon` + native `HMENU`).
//!
//! **Status: M3 — requires verification on Windows.** The channel + bridge task
//! dispatch architecture mirrors the other backends; the native `tray-icon` wiring
//! below is a faithful starting point that may need minor adjustments against the
//! pinned `tray-icon` release.

#![cfg(target_os = "windows")]

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

use parking_lot::Mutex;

use flume::Sender;
use gpui::{App, AsyncApp, SharedString};
use tray_icon::{
    TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, Submenu},
};

use crate::backend::TrayBackend;
use crate::event::TrayEvent;
use crate::icon::TrayIconData;
use crate::LeftClick;
use crate::menu::{split_menu, MenuItemId, TrayMenu, TrayMenuItem};
use crate::AnyAction;

pub struct WindowsBackend {
    #[allow(dead_code)]
    tray: tray_icon::TrayIcon,
    shared: Arc<Mutex<WinShared>>,
    tx: Sender<TrayEvent>,
}

struct WinShared {
    handlers: HashMap<MenuItemId, crate::menu::Handler>,
    left_click: LeftClick,
    available: bool,
}

fn build_menu(items: &[TrayMenuItem]) -> Menu {
    let mut menu = Menu::new();
    for item in items {
        match item {
            TrayMenuItem::Separator => {
                menu.append(&tray_icon::menu::MenuItem::separator());
            }
            TrayMenuItem::Submenu(sub) => {
                let submenu = build_menu(&sub.items);
                let _ = menu.append(&Submenu::with_id(
                    sub.label.to_string(),
                    sub.label.to_string(),
                    true,
                    &[&submenu],
                ));
            }
            TrayMenuItem::Item(it) => {
                let id = it.id.0.to_string();
                let _ = menu.append(&MenuItem::with_id(
                    id,
                    it.label.to_string(),
                    it.enabled,
                    None::<&str>,
                ));
            }
        }
    }
    menu
}

pub fn create_backend(cx: &mut App) -> anyhow::Result<Box<dyn TrayBackend>> {
    let (tx, rx) = flume::unbounded::<TrayEvent>();

    // `tray-icon` reports menu clicks through a global receiver; pump it on a
    // dedicated thread and forward to the bridge over the (Send) flume channel.
    thread::spawn(move || {
        while let Ok(event) = MenuEvent::receiver().recv() {
            let id = MenuItemId(SharedString::from(event.id.to_string()));
            let _ = tx.send(TrayEvent::MenuItemActivated { id });
        }
    });

    let tray_menu = build_menu(&[]);
    let tray = TrayIconBuilder::new()
        .with_menu(tray_menu)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to create tray icon: {e}"))?;

    let shared = Arc::new(Mutex::new(WinShared {
        handlers: HashMap::new(),
        left_click: LeftClick::Menu,
        available: true,
    }));

    let app = cx.to_async();
    let shared_task = Arc::clone(&shared);
    let _task = cx.spawn(|_cx: &mut AsyncApp| {
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

    Ok(WindowsBackend { tray, shared, tx })
}

impl TrayBackend for WindowsBackend {
    fn is_available(&self) -> bool {
        self.shared.lock().available
    }

    fn set_visible(&self, visible: bool) {
        self.shared.lock().available = visible;
        self.tray.set_visible(visible);
    }

    fn set_icon(&self, icon: Option<TrayIconData>) {
        if let Some(icon) = icon {
            // `tray-icon` expects BGRA; convert from the tray's sRGBA. A production
            // build should also cache and reuse the `Icon` handle.
            let rgba = icon.rgba;
            let _ = self.tray.set_icon(Some(
                tray_icon::icon::Icon::from_rgba(rgba, icon.width, icon.height)
                    .map_err(|e| anyhow::anyhow!("icon: {e}")),
            ));
        }
    }

    fn set_tooltip(&self, tooltip: Option<SharedString>) {
        if let Some(tip) = tooltip {
            let _ = self.tray.set_tooltip(Some(tip.to_string()));
        }
    }

    fn set_menu(&self, menu: TrayMenu) {
        let (menu, handlers) = split_menu(menu);
        let native = build_menu(&menu.items);
        self.tray.set_menu(Some(native));
        self.shared.lock().handlers = handlers;
    }

    fn set_left_click_action(&self, behavior: LeftClick) {
        self.shared.lock().left_click = behavior;
    }
}
