//! macOS backend: `objc2` `NSStatusItem` (native menu bar item).
//!
//! The status item and its menu live on the main thread, which is the thread
//! the GPUI application runs on. The click handler is an Objective-C class
//! defined here; it forwards activations to the bridge task over a channel.
//! The bridge task is owned by the backend, so `shutdown` stops the loop.

#![cfg(target_os = "macos")]

use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker};
use objc2_app_kit::{
    NSBitmapImageRep, NSCalibratedRGBColorSpace, NSControlStateValueOff, NSControlStateValueOn,
    NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{NSSize, NSString};

use flume::Sender;
use gpui::{App, AsyncApp, Keystroke, Modifiers, SharedString};

use crate::backend::TrayBackend;
use crate::error::TrayError;
use crate::event::{TrayButton, TrayButtonState, TrayEvent};
use crate::icon::TrayIcon;
use crate::menu::{ResolvedEntry, ResolvedMenu, TrayItemId};
use crate::options::TrayOptions;
use crate::runtime::{route_event, BackendSignal, RuntimeShared};

/// The instance variables of the click handler.
struct MacHandlerIvars {
    sender: Sender<BackendSignal>,
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
                    let _ = ivars.sender.send(BackendSignal::Event(
                        TrayEvent::MenuItemActivated {
                            id: TrayItemId::new(id.to_string()),
                        },
                    ));
                }
            }
        }

        #[unsafe(method(statusItemClicked:))]
        fn status_clicked(&self, _sender: Option<&NSObject>) {
            let ivars = self.ivars();
            let _ = ivars.sender.send(BackendSignal::Event(TrayEvent::Activated {
                button: TrayButton::Left,
                state: TrayButtonState::Up,
                position: crate::runtime::make_point(0.0, 0.0, 1.0),
                rect: None,
            }));
        }
    }
);

impl MacHandler {
    fn new(sender: Sender<BackendSignal>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(MacHandlerIvars { sender });
        // SAFETY: `NSObject`'s `init` is its designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

pub struct MacBackend {
    status_item: Retained<NSStatusItem>,
    handler: Retained<MacHandler>,
    shared: Arc<RuntimeShared>,
    // Held so the bridge task lives as long as the backend; dropped when the
    // backend is dropped, which cancels the loop.
    _bridge: gpui::Task<()>,
}

/// Convert sRGBA bytes into an `NSImage` sized in logical points.
fn make_ns_image(icon: &crate::icon::TrayIconBitmap) -> Option<Retained<NSImage>> {
    let mtm = MainThreadMarker::new()?;
    let image = icon.images().first()?;
    let pixels = image.pixel_width() as isize;
    let rows = image.pixel_height() as isize;
    // Allocate the representation with its own buffer, then copy the sRGBA
    // bytes in — handing AppKit a foreign buffer would risk a dangling read.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            pixels,
            rows,
            8,
            4,
            true,
            false,
            NSCalibratedRGBColorSpace,
            pixels * 4,
            32,
        )
    }?;
    unsafe {
        let dst = rep.bitmapData();
        if !dst.is_null() {
            std::ptr::copy_nonoverlapping(image.rgba().as_ptr(), dst, image.rgba().len());
        }
    }
    let ns_image = NSImage::new();
    ns_image.addRepresentation(&rep);
    // Logical size in points so the icon renders at the right bar height on
    // Retina displays.
    ns_image.setSize(NSSize::new(
        image.width() as f64,
        image.height() as f64,
    ));
    Some(ns_image)
}

fn build_menu(
    menu: &ResolvedMenu,
    handler: &MacHandler,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let ns_menu = NSMenu::new(mtm);
    // AppKit must not re-evaluate `enabled` on open, or items the application
    // disabled through the model get re-enabled.
    ns_menu.setAutoenablesItems(false);
    fill_menu(&ns_menu, &menu.entries, handler, mtm);
    ns_menu
}

fn fill_menu(
    ns_menu: &NSMenu,
    entries: &[ResolvedEntry],
    handler: &MacHandler,
    mtm: MainThreadMarker,
) {
    for entry in entries {
        match entry {
            ResolvedEntry::Separator => {
                ns_menu.addItem(&NSMenuItem::separatorItem(mtm));
            }
            ResolvedEntry::Submenu(sub) => {
                let submenu = NSMenu::new(mtm);
                submenu.setAutoenablesItems(false);
                fill_menu(&submenu, &sub.entries(), handler, mtm);
                let parent = NSMenuItem::new(mtm);
                parent.setTitle(&NSString::from_str(sub.label().as_ref()));
                parent.setSubmenu(Some(&submenu));
                ns_menu.addItem(&parent);
            }
            ResolvedEntry::Item(item) => {
                let mi = NSMenuItem::new(mtm);
                mi.setTitle(&NSString::from_str(item.label().as_ref()));
                mi.setEnabled(item.is_enabled());
                match item.checked() {
                    Some(true) => mi.setState(NSControlStateValueOn),
                    Some(false) => mi.setState(NSControlStateValueOff),
                    None => {}
                }
                if let Some(accelerator) = item.accelerator() {
                    let (key, mask) = key_equivalent(accelerator);
                    mi.setKeyEquivalent(&NSString::from_str(&key));
                    mi.setKeyEquivalentModifierMask(mask);
                }
                let represented = NSString::from_str(item.id().as_str());
                // SAFETY: the represented object is an `NSString`, and the
                // target is the handler defined above, which outlives the menu.
                unsafe {
                    mi.setRepresentedObject(Some(&represented));
                    mi.setAction(Some(sel!(menuItemClicked:)));
                    mi.setTarget(Some(handler));
                }
                ns_menu.addItem(&mi);
            }
        }
    }
}

/// Map a GPUI `Keystroke` to an AppKit key equivalent.
fn key_equivalent(k: &Keystroke) -> (String, objc2_app_kit::NSEventModifierFlags) {
    let modifiers: Modifiers = k.modifiers;
    let mut flags = objc2_app_kit::NSEventModifierFlags(0);
    if modifiers.control {
        flags |= objc2_app_kit::NSEventModifierFlags::Control;
    }
    if modifiers.alt {
        flags |= objc2_app_kit::NSEventModifierFlags::Option;
    }
    if modifiers.shift {
        flags |= objc2_app_kit::NSEventModifierFlags::Shift;
    }
    if modifiers.platform {
        flags |= objc2_app_kit::NSEventModifierFlags::Command;
    }
    (k.key.to_lowercase(), flags)
}

pub fn create_backend(
    cx: &mut App,
    options: &TrayOptions,
    shared: Arc<RuntimeShared>,
) -> Result<MacBackend, TrayError> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err(TrayError::WrongThread);
    };

    let (tx, rx) = flume::unbounded::<BackendSignal>();
    let status_bar = NSStatusBar::systemStatusBar();
    let status_item = status_bar.statusItemWithLength(objc2_app_kit::NSVariableStatusItemLength);

    let handler = MacHandler::new(tx);

    let app = cx.to_async();
    let shared_task = Arc::clone(&shared);
    let bridge = cx.spawn(move |_cx: &mut AsyncApp| {
        let app = app.clone();
        let shared = shared_task;
        let rx = rx;
        async move {
            while let Ok(signal) = rx.recv_async().await {
                if shared.is_closed() {
                    break;
                }
                match signal {
                    BackendSignal::Event(event) => route_event(&shared, &app, event),
                    BackendSignal::Refresh | BackendSignal::Shutdown => break,
                }
            }
        }
    });

    // Availability is decided at creation; the menu bar always has room.
    shared.set_availability(crate::error::TrayAvailability::Available);

    // Apply the creation-time options.
    if let Some(icon) = options.icon() {
        let _ = set_icon_impl(&status_item, Some(icon));
    }
    if let Some(tooltip) = options.tooltip() {
        if let Some(button) = status_item.button(mtm) {
            button.setToolTip(Some(&NSString::from_str(tooltip.as_ref())));
        }
    }
    if let Some(title) = options.title() {
        if let Some(button) = status_item.button(mtm) {
            button.setTitle(&NSString::from_str(title.as_ref()));
        }
    }

    // Wire the left-click behavior.
    let menu = build_menu(&ResolvedMenu::empty(), &handler, mtm);
    match &options.left_click {
        crate::LeftClick::Menu => {
            status_item.setMenu(Some(&menu));
        }
        crate::LeftClick::None => {
            status_item.setMenu(None);
        }
        crate::LeftClick::Action(_) => {
            status_item.setMenu(None);
            if let Some(button) = status_item.button(mtm) {
                // SAFETY: the handler outlives the status item.
                unsafe {
                    button.setAction(Some(sel!(statusItemClicked:)));
                    button.setTarget(Some(&handler));
                }
                button.sendActionOn(objc2_app_kit::NSEventMask::LeftMouseUp);
            }
        }
    }

    Ok(MacBackend {
        status_item,
        handler,
        shared,
        _bridge: bridge,
    })
}

fn set_icon_impl(status_item: &NSStatusItem, icon: Option<&TrayIcon>) -> Result<(), TrayError> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err(TrayError::WrongThread);
    };
    let Some(button) = status_item.button(mtm) else {
        return Err(TrayError::Native("status item has no button".into()));
    };
    match icon {
        Some(TrayIcon::Bitmap(bitmap)) => {
            let Some(ns_image) = make_ns_image(bitmap) else {
                return Err(TrayError::InvalidIcon("failed to build NSImage".into()));
            };
            ns_image.setTemplate(bitmap.is_templated());
            button.setImage(Some(&ns_image));
        }
        Some(TrayIcon::Named(name)) => {
            // Try the system named-image catalog; a miss is reported instead
            // of silently leaving the old icon in place.
            let image = NSImage::imageNamed(&NSString::from_str(name.as_str()));
            if let Some(image) = image {
                button.setImage(Some(&image));
            } else {
                return Err(TrayError::Native(format!(
                    "named image `{}` not found",
                    name.as_str()
                ).into()));
            }
        }
        None => {
            button.setImage(None);
        }
    }
    Ok(())
}

impl TrayBackend for MacBackend {
    fn is_available(&self) -> bool {
        self.shared.availability().is_available()
    }

    fn set_visible(&self, visible: bool) -> Result<(), TrayError> {
        self.status_item.setVisible(visible);
        Ok(())
    }

    fn set_icon(&self, icon: Option<TrayIcon>) -> Result<(), TrayError> {
        set_icon_impl(&self.status_item, icon.as_ref())
    }

    fn set_tooltip(&self, tooltip: Option<SharedString>) -> Result<(), TrayError> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(TrayError::WrongThread);
        };
        let Some(button) = self.status_item.button(mtm) else {
            return Err(TrayError::Native("status item has no button".into()));
        };
        match tooltip {
            Some(tip) => button.setToolTip(Some(&NSString::from_str(tip.as_ref()))),
            None => button.setToolTip(None),
        }
        Ok(())
    }

    fn set_title(&self, title: Option<SharedString>) -> Result<(), TrayError> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(TrayError::WrongThread);
        };
        let Some(button) = self.status_item.button(mtm) else {
            return Err(TrayError::Native("status item has no button".into()));
        };
        match title {
            Some(t) => button.setTitle(&NSString::from_str(t.as_ref())),
            None => button.setTitle(&NSString::from_str("")),
        }
        Ok(())
    }

    fn set_menu(&self, menu: &ResolvedMenu) -> Result<(), TrayError> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(TrayError::WrongThread);
        };
        let ns_menu = build_menu(menu, &self.handler, mtm);
        // Replace the menu only when it is closed; if a menu is open, the
        // next open picks up the new snapshot.
        self.status_item.setMenu(Some(&ns_menu));
        Ok(())
    }

    fn show_menu(&self) -> Result<(), TrayError> {
        Err(TrayError::UnsupportedPlatform)
    }

    fn request_attention(&self, _attention: bool) -> Result<(), TrayError> {
        Err(TrayError::UnsupportedPlatform)
    }

    fn shutdown(&self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.status_item);
        self.shared.clear_handlers();
    }
}
