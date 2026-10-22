//! macOS backend: `objc2` `NSStatusItem` (native menu bar item).
//!
//! The status item and its menu live on the main thread, which is the thread
//! the GPUI application runs on. The handler that receives menu clicks is an
//! Objective-C class defined here; it forwards the click to the bridge task
//! over a channel.

#![cfg(target_os = "macos")]

use std::collections::HashMap;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker};
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem};
use objc2_foundation::NSString;

use flume::Sender;
use gpui::{App, AsyncApp, SharedString};
use parking_lot::Mutex;

use crate::backend::TrayBackend;
use crate::event::TrayEvent;
use crate::icon::TrayIconData;
use crate::menu::{split_menu, MenuItemId, TrayMenu, TrayMenuItem};
use crate::AnyAction;
use crate::LeftClick;

/// The instance variables of the menu click handler.
struct MacHandlerIvars {
    sender: Sender<TrayEvent>,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and the handler
    // does not implement `Drop`, so `define_class!` releases its ivars.
    #[unsafe(super(NSObject))]
    #[ivars = MacHandlerIvars]
    struct MacHandler;

    impl MacHandler {
        #[unsafe(method(menuItemClicked:))]
        fn clicked(&self, item: &NSMenuItem) {
            let ivars = self.ivars();
            if let Some(object) = item.representedObject() {
                if let Some(id) = object.downcast_ref::<NSString>() {
                    let _ = ivars.sender.send(TrayEvent::MenuItemActivated {
                        id: MenuItemId(SharedString::from(id.to_string())),
                    });
                }
            }
        }
    }
);

impl MacHandler {
    fn new(sender: Sender<TrayEvent>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(MacHandlerIvars { sender });
        // SAFETY: `NSObject`'s `init` is its designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

pub struct MacBackend {
    status_item: Retained<NSStatusItem>,
    handler: Retained<MacHandler>,
    shared: Arc<Mutex<MacShared>>,
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

fn build_menu(
    items: &[TrayMenuItem],
    handler: &MacHandler,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for item in items {
        match item {
            TrayMenuItem::Separator => {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
            }
            TrayMenuItem::Submenu(sub) => {
                let submenu = build_menu(&sub.items, handler, mtm);
                let parent = NSMenuItem::new(mtm);
                parent.setTitle(&NSString::from_str(sub.label.as_ref()));
                parent.setSubmenu(Some(&submenu));
                menu.addItem(&parent);
            }
            TrayMenuItem::Item(it) => {
                let mi = NSMenuItem::new(mtm);
                mi.setTitle(&NSString::from_str(it.label.as_ref()));
                mi.setEnabled(it.enabled);
                if let Some(true) = it.checked {
                    mi.setState(objc2_app_kit::NSControlStateValueOn);
                }
                let represented = NSString::from_str(it.id.0.as_ref());
                // SAFETY: the represented object is an `NSString`, and the
                // target is the handler defined above, which outlives the menu.
                unsafe {
                    mi.setRepresentedObject(Some(&represented));
                    mi.setAction(Some(sel!(menuItemClicked:)));
                    mi.setTarget(Some(handler));
                }
                menu.addItem(&mi);
            }
        }
    }
    menu
}

pub fn create_backend(cx: &mut App) -> anyhow::Result<MacBackend> {
    let Some(mtm) = MainThreadMarker::new() else {
        anyhow::bail!("the system tray needs the main thread, which the application is not on");
    };

    let (tx, rx) = flume::unbounded::<TrayEvent>();
    let status_bar = NSStatusBar::systemStatusBar();
    let status_item = status_bar.statusItemWithLength(objc2_app_kit::NSVariableStatusItemLength);

    let handler = MacHandler::new(tx);

    let shared = Arc::new(Mutex::new(MacShared {
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

    // Build an initial (empty) menu so the status item is interactive.
    let menu = build_menu(&[], &handler, mtm);
    status_item.setMenu(Some(&menu));

    Ok(MacBackend {
        status_item,
        handler,
        shared,
    })
}

impl TrayBackend for MacBackend {
    fn is_available(&self) -> bool {
        self.shared.lock().available
    }

    fn set_visible(&self, visible: bool) {
        self.shared.lock().available = visible;
        self.status_item.setVisible(visible);
    }

    fn set_icon(&self, icon: Option<TrayIconData>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if let Some(icon) = icon {
            let ns_image = make_ns_image(&icon);
            ns_image.setTemplate(true);
            if let Some(button) = self.status_item.button(mtm) {
                button.setImage(Some(&ns_image));
            }
        } else if let Some(button) = self.status_item.button(mtm) {
            button.setImage(None);
        }
    }

    fn set_tooltip(&self, tooltip: Option<SharedString>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if let Some(tip) = tooltip {
            if let Some(button) = self.status_item.button(mtm) {
                button.setToolTip(Some(&NSString::from_str(tip.as_ref())));
            }
        }
    }

    fn set_menu(&self, menu: TrayMenu) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let (menu, handlers) = split_menu(menu);
        let ns_menu = build_menu(&menu.items, &self.handler, mtm);
        self.status_item.setMenu(Some(&ns_menu));
        self.shared.lock().handlers = handlers;
    }

    fn set_left_click_action(&self, behavior: LeftClick) {
        self.shared.lock().left_click = behavior;
    }
}
