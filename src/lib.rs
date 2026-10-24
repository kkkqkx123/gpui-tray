//! # gpui-tray
//!
//! System tray / menu-bar icon support for [GPUI](https://gpui.rs) applications.
//!
//! The crate provides a small, cross-platform [`Tray`] handle. Menu items carry
//! either a GPUI [`Action`](gpui::Action) (dispatched through the normal GPUI
//! action pipeline) or a plain callback, so tray interaction looks exactly like
//! the app menu / keybinding paths already in GPUI.
//!
//! ## Backends
//!
//! | Platform | Backend                              |
//! | -------- | ------------------------------------ |
//! | Linux    | `ksni` (StatusNotifierItem / DBus)   |
//! | macOS    | `objc2` NSStatusItem (native)        |
//! | Windows  | `tray-icon` (Shell_NotifyIcon)       |
//!
//! If the platform tray is unavailable (e.g. GNOME without the AppIndicator
//! extension) [`Tray::availability`] reports [`TrayAvailability::Unavailable`]
//! and the application starts normally — the tray never blocks startup. Hiding
//! the icon with [`Tray::set_visible`] never changes availability.
//!
//! ## GPUI flavors
//!
//! Select exactly one flavor: `gpui-kit` (GPUI Kit's `gpui-pre` types) or
//! `gpui-fast` (GPUI Fast). Disable default features when selecting an
//! alternative. The selected flavor must be the GPUI the application itself
//! builds against, because every operation takes that flavor's `gpui::App`.
//!
//! ## Example
//!
//! ```ignore
//! use gpui_kit::*;
//! use gpui_tray::*;
//!
//! actions!(demo, [ShowWindow, Quit]);
//!
//! fn main() {
//!     gpui_kit::application().run(|cx| {
//!         gpui_kit::init(cx);
//!         let tray = Tray::new(
//!             cx,
//!             TrayOptions::builder("demo.tray")
//!                 .with_tooltip("Demo")
//!                 .build(),
//!         )
//!         .expect("tray");
//!         tray.set_menu(
//!             cx,
//!             tray_menu(|m| {
//!                 m.action(ShowWindow, "Show Window");
//!                 m.separator();
//!                 m.action(Quit, "Quit");
//!             }),
//!         )
//!         .expect("menu");
//!     });
//! }
//! ```

#[cfg(all(feature = "gpui-kit", feature = "gpui-fast"))]
compile_error!(
    "features `gpui-kit` and `gpui-fast` are mutually exclusive; use default-features = false and select exactly one backend"
);

#[cfg(not(any(feature = "gpui-kit", feature = "gpui-fast")))]
compile_error!("enable exactly one of the `gpui-kit` or `gpui-fast` features");

// Both flavors arrive under a renamed dependency, so the selected one takes
// the name `gpui` here.
#[cfg(all(feature = "gpui-kit", not(feature = "gpui-fast")))]
extern crate gpui_kit_backend as gpui;
#[cfg(all(feature = "gpui-fast", not(feature = "gpui-kit")))]
extern crate gpui_fast_backend as gpui;

pub mod backend;
pub mod error;
pub mod event;
pub mod icon;
pub mod menu;
mod options;
pub mod platform;
mod runtime;

pub use backend::TrayBackend;
pub use error::{TrayAvailability, TrayError, TrayUnavailableReason};
pub use event::{
    TrayButton, TrayButtonState, TrayEvent, TrayPoint, TrayRect, TrayScrollOrientation, TraySize,
};
pub use icon::{TrayIcon, TrayIconBitmap, TrayIconBitmapBuilder, TrayIconImage, TrayIconName};
pub use menu::{
    tray_menu, AnyAction, ResolvedEntry, ResolvedItem, ResolvedMenu, ResolvedSubmenu, TrayEntry,
    TrayItem, TrayItemBuilder, TrayItemId, TrayMenu, TrayMenuBuilder, TraySubmenu,
};
pub use options::{TrayId, TrayOptions, TrayOptionsBuilder};
pub use platform::PlatformTrayBackend;

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{App, Entity, SharedString, Subscription};

use runtime::{RuntimeShared, TrayRuntime};

/// Behavior triggered when the tray icon is activated (typically a left click).
///
/// Fixed at creation time through [`TrayOptionsBuilder::with_left_click`]:
/// the Linux backend bakes the choice into the StatusNotifierItem at
/// registration, so it cannot change while the tray exists. To change the
/// behavior, [`Tray::remove`] the tray and create a new one.
pub enum LeftClick {
    /// The left click does nothing.
    None,
    /// Open the tray menu. On Linux the desktop shell draws the menu; on
    /// macOS/Windows the left click opens it. The menu stays reachable via
    /// right click.
    Menu,
    /// Dispatch an action when the tray icon is activated.
    Action(AnyAction),
}

/// Clone support: the stored action is shared behind an `Arc`.
impl Clone for LeftClick {
    fn clone(&self) -> Self {
        match self {
            LeftClick::None => LeftClick::None,
            LeftClick::Menu => LeftClick::Menu,
            LeftClick::Action(a) => LeftClick::Action(Arc::clone(a)),
        }
    }
}

/// A handle to the system tray.
///
/// Owns the runtime: the platform backend, its bridge task, the dispatch
/// handler table and the availability state. Cheaply cloneable — clones share
/// one runtime. The tray belongs to the main thread, like every other handle
/// into application state; the type is deliberately `!Send` so platform
/// threading constraints are enforced at compile time. Must be created inside
/// `application().run(|cx| …)` on the main thread.
pub struct Tray {
    runtime: Arc<TrayRuntime>,
    _main_thread: PhantomData<Rc<()>>,
}

impl Clone for Tray {
    fn clone(&self) -> Self {
        Self {
            runtime: Arc::clone(&self.runtime),
            _main_thread: PhantomData,
        }
    }
}

impl Tray {
    /// Create the tray on the current platform.
    ///
    /// Never blocks on desktop startup. If the platform tray is unavailable,
    /// [`Tray::availability`] reports it and the app runs normally.
    pub fn new(cx: &mut App, options: TrayOptions) -> Result<Self, TrayError> {
        let shared = Arc::new(RuntimeShared::new(&options));
        let backend = PlatformTrayBackend::new(cx, &options, Arc::clone(&shared))?;
        Ok(Self {
            runtime: Arc::new(TrayRuntime {
                options,
                backend,
                shared,
            }),
            _main_thread: PhantomData,
        })
    }

    /// The stable identifier of this tray.
    pub fn id(&self) -> &TrayId {
        &self.runtime.options.id
    }

    /// The current availability state.
    pub fn availability(&self) -> TrayAvailability {
        self.runtime.shared.availability()
    }

    /// Whether a working tray is present on this platform/session. True only
    /// while availability is [`TrayAvailability::Available`].
    pub fn is_available(&self) -> bool {
        self.availability().is_available()
    }

    /// Whether the icon is currently shown. Independent of availability.
    pub fn is_visible(&self) -> bool {
        self.runtime.shared.is_visible()
    }

    /// The last reported icon rectangle, when the platform provides one
    /// (Windows). Linux does not report the rectangle.
    pub fn rect(&self) -> Option<TrayRect> {
        self.runtime.shared.rect()
    }

    /// Subscribe to tray events. The handler runs on the main thread, after
    /// any configured action/callback dispatch.
    pub fn on_event(
        &self,
        _cx: &mut App,
        handler: impl FnMut(TrayEvent, &mut App) + Send + 'static,
    ) -> Subscription {
        let shared = Arc::clone(&self.runtime.shared);
        let listener_id = shared.add_listener(Box::new(handler));
        let release_shared = Arc::clone(&shared);
        Subscription::new(move || {
            release_shared.remove_listener(listener_id);
        })
    }

    /// Show or hide the tray icon.
    ///
    /// Visibility never changes availability: a hidden tray still reports
    /// [`TrayAvailability::Available`].
    pub fn set_visible(&self, _cx: &mut App, visible: bool) -> Result<(), TrayError> {
        self.runtime.shared.set_visible(visible);
        self.backend().set_visible(visible)
    }

    /// Set the tray icon (or clear it with `None`).
    pub fn set_icon(&self, _cx: &mut App, icon: Option<TrayIcon>) -> Result<(), TrayError> {
        self.backend().set_icon(icon)
    }

    /// Set the hover tooltip text (or clear it with `None`).
    pub fn set_tooltip(&self, _cx: &mut App, tooltip: Option<SharedString>) -> Result<(), TrayError> {
        self.backend().set_tooltip(tooltip)
    }

    /// Set the text shown next to the icon where the platform supports it
    /// (Linux, macOS).
    pub fn set_title(&self, _cx: &mut App, title: Option<SharedString>) -> Result<(), TrayError> {
        self.backend().set_title(title)
    }

    /// Replace the entire menu.
    ///
    /// The menu is resolved into a render snapshot plus a handler table before
    /// it reaches the backend. Duplicate item ids are rejected here.
    pub fn set_menu(&self, _cx: &mut App, menu: TrayMenu) -> Result<(), TrayError> {
        let (resolved, handlers) = menu::resolve_menu(menu)?;
        self.runtime.shared.replace_handlers(handlers);
        self.backend().set_menu(&resolved)
    }

    /// Bind the menu to an entity: the menu is rebuilt from the supplied
    /// closure every time the entity notifies, so checked states and labels
    /// follow application state without manual `set_menu` calls.
    ///
    /// macOS rebuilds the native menu on each notify; if a menu is open the
    /// update applies on its next open. On Windows an open native menu keeps
    /// its contents until the next open.
    pub fn bind_menu<T: 'static>(
        &self,
        cx: &mut App,
        entity: Entity<T>,
        build: impl Fn(&T, &mut App) -> TrayMenu + Send + 'static,
    ) -> Subscription {
        let this = self.clone();
        cx.observe(&entity, move |entity, cx| {
            entity.update(cx, |state, cx| {
                let menu = build(state, cx);
                let _ = this.set_menu(cx, menu);
            });
        })
    }

    /// Open the menu programmatically, where the platform supports it.
    pub fn show_menu(&self, _cx: &mut App) -> Result<(), TrayError> {
        self.backend().show_menu()
    }

    /// Mark the icon as requesting attention, where the platform supports it
    /// (Linux `NeedsAttention`).
    pub fn request_attention(&self, _cx: &mut App, attention: bool) -> Result<(), TrayError> {
        self.backend().request_attention(attention)
    }

    /// Remove the tray deterministically: the native icon disappears, the
    /// bridge task stops, and no further events are delivered. Subsequent
    /// operations on any clone report [`TrayError::Closed`].
    ///
    /// Dropping the last [`Tray`] handle also cancels the bridge task, but
    /// native icon removal is best effort then; call `remove` when the tray
    /// must disappear at a defined point (e.g. application quit).
    pub fn remove(self, _cx: &mut App) {
        self.runtime.shutdown();
    }

    fn backend(&self) -> &PlatformTrayBackend {
        // Late events for a closed tray are dropped by the shared state; the
        // native calls themselves are harmless after shutdown.
        &self.runtime.backend
    }
}
