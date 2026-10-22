//! macOS backend: native `NSStatusItem` via `objc2` (aligned with the `objc2` 0.6 /
//! `objc2-app-kit` 0.3.2 versions already linked by `gpui-pre-macos`).
//!
//! **Status: M2 — requires verification on a macOS device.** The architecture
//! (channel + bridge task dispatching GPUI actions on the main thread) mirrors
//! the Linux backend; the native `objc2` wiring below is a faithful starting
//! point that may need minor API adjustments against the pinned `objc2` release.

#![cfg(target_os = "macos")]

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use flume::Sender;
use gpui::{App, AsyncApp, SharedString};
use objc2::declare_class;
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::NSString;

use crate::backend::TrayBackend;
use crate::event::TrayEvent;
use crate::icon::TrayIconData;
use crate::LeftClick;
use crate::menu::{split_menu, MenuItemId, TrayMenu, TrayMenuItem};
use crate::AnyAction;

#[derive(Debug, Default)]
struct MacHandlerIvars {
    sender: Sender<TrayEvent>,
}

declare_class!(
    struct MacHandler;
    unsafe impl ClassType for MacHandler {
        type Super = NSObject;
        name = "GpuiTrayMacHandler";
        ivars = MacHandlerIvars;
    }
    unsafe impl MacHandler {
        #[method(menuItemClicked:)]
        fn clicked(&self, item: &NSMenuItem) {
            let ivars = self.ivars();
            if let Some(obj) = item.representedObject() {
                if let Some(s) = obj.as_ref().downcast_ref::<NSString>() {
                    let _ = ivars
                        .sender
                        .send(TrayEvent::MenuItemActivated {
                            id: MenuItemId(SharedString::from(s.to_string())),
                        });
                }
            }
        }
    }
);

pub struct MacBackend {
    #[allow(dead_code)]
    status_item: Retained<NSStatusItem>,
    handler: Retained<MacHandler>,
    shared: Arc<Mutex<MacShared>>,
    tx: Sender<TrayEvent>,
}

struct MacShared {
    handlers: HashMap<MenuItemId, crate::menu::Handler>,
    left_click: LeftClick,
    available: bool,
}

fn make_ns_image(_icon: &TrayIconData) -> Retained<NSImage> {
    // A full NSBitmapImageRep wiring is intentionally minimal here; the production
    // path should rasterize the `TrayIconData` (or an SVG) into an `NSImage`.
    // Callers pass a single-color icon so `setTemplate(true)` adapts to the
    // light/dark menu bar automatically.
    NSImage::new()
}

fn build_menu(items: &[TrayMenuItem], handler: &MacHandler, tx: &Sender<TrayEvent>) -> Retained<NSMenu> {
    let menu = NSMenu::new();
    for item in items {
        match item {
            TrayMenuItem::Separator => {
                menu.addItem(&NSMenuItem::separatorItem());
            }
            TrayMenuItem::Submenu(sub) => {
                let submenu = build_menu(&sub.items, handler, tx);
                let mut parent = NSMenuItem::new();
                parent.setTitle(&NSString::from_str(&sub.label.to_string()));
                parent.setSubmenu(Some(&submenu));
                menu.addItem(&parent);
            }
            TrayMenuItem::Item(it) => {
                let mut mi = NSMenuItem::new();
                mi.setTitle(&NSString::from_str(&it.label.to_string()));
                mi.setEnabled(it.enabled);
                if let Some(true) = it.checked {
                    mi.setState(objc2_app_kit::NSControlStateValueOn);
                }
                mi.setRepresentedObject(Some(&NSString::from_str(&it.id.0.to_string())));
                mi.setAction(Some(objc2::sel!(menuItemClicked:)));
                mi.setTarget(Some(handler));
                menu.addItem(&mi);
            }
        }
    }
    menu
}

pub fn create_backend(cx: &mut App) -> anyhow::Result<Box<dyn TrayBackend>> {
    let (tx, rx) = flume::unbounded::<TrayEvent>();
    let status_bar = NSStatusBar::systemStatusBar();
    let status_item = status_bar.statusItemWithLength(objc2_app_kit::NSVariableStatusItemLength);

    let handler = MacHandler::new(MacHandlerIvars { sender: tx.clone() });

    let shared = Arc::new(Mutex::new(MacShared {
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

    // Build an initial (empty) menu so the status item is interactive.
    let menu = build_menu(&[], &handler, &tx);
    status_item.setMenu(Some(&menu));

    Ok(MacBackend {
        status_item,
        handler,
        shared,
        tx,
    })
}

impl TrayBackend for MacBackend {
    fn is_available(&self) -> bool {
        self.shared.lock().available
    }

    fn set_visible(&self, visible: bool) {
        self.shared.lock().available = visible;
        self.status_item.button().map(|b| b.setVisible(visible));
    }

    fn set_icon(&self, icon: Option<TrayIconData>) {
        if let Some(icon) = icon {
            let ns_image = make_ns_image(&icon);
            ns_image.setTemplate(true);
            self.status_item.button().map(|b| b.setImage(Some(&ns_image)));
        } else {
            self.status_item.button().map(|b| b.setImage(None));
        }
    }

    fn set_tooltip(&self, tooltip: Option<SharedString>) {
        if let Some(tip) = tooltip {
            self.status_item.button().map(|b| b.setToolTip(Some(&NSString::from_str(&tip.to_string()))));
        }
    }

    fn set_menu(&self, menu: TrayMenu) {
        let (menu, handlers) = split_menu(menu);
        let ns_menu = build_menu(&menu.items, &self.handler, &self.tx);
        self.status_item.setMenu(Some(&ns_menu));
        self.shared.lock().handlers = handlers;
    }

    fn set_left_click_action(&self, behavior: LeftClick) {
        self.shared.lock().left_click = behavior;
    }
}
