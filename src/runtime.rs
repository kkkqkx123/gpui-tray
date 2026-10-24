//! The tray runtime: the single owner of dispatch handlers, availability
//! state, subscriptions and the bridge task.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use gpui::{App, AsyncApp};
use parking_lot::Mutex;

use crate::backend::TrayBackend as _;
use crate::error::TrayAvailability;
use crate::event::TrayEvent;
use crate::event::TrayRect;
use crate::event::TrayPoint;
use crate::event::TraySize;
use crate::menu::{AnyAction, TrayHandlerMap, TrayItemId};
use crate::options::TrayOptions;
use crate::LeftClick;

/// A signal sent from a native backend thread to the bridge task.
///
/// [`BackendSignal::Event`] is a public [`TrayEvent`]; [`BackendSignal::Refresh`]
/// is an internal command (Linux nudges the StatusNotifierItem to re-read its
/// state) and never reaches the application.
pub(crate) enum BackendSignal {
    Event(TrayEvent),
    Refresh,
    /// Tear the native service down and exit the bridge loop.
    Shutdown,
}

pub(crate) type Listener = Box<dyn FnMut(TrayEvent, &mut App) + Send>;

/// State shared between the runtime, the bridge task and the native threads
/// that report events.
pub(crate) struct RuntimeShared {
    closed: AtomicBool,
    availability: Mutex<TrayAvailability>,
    visible: AtomicBool,
    /// The action dispatched on tray activation, when the tray was created
    /// with `LeftClick::Action`.
    left_action: Option<AnyAction>,
    handlers: Mutex<TrayHandlerMap>,
    listeners: Mutex<HashMap<u64, Listener>>,
    next_listener_id: AtomicU64,
    /// The last reported icon rectangle; `None` where the platform does not
    /// report one (Linux).
    // Written by the Windows event pump; dead on other targets.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    rect: Mutex<Option<TrayRect>>,
    /// The bridge task's receive side is paired with this sender; backends
    /// without a direct handle to the bridge use it to deliver events.
    // Used by the Windows global router; dead on other targets.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    event_sender: Mutex<Option<flume::Sender<BackendSignal>>>,
}

impl RuntimeShared {
    pub(crate) fn new(options: &TrayOptions) -> Self {
        Self {
            closed: AtomicBool::new(false),
            availability: Mutex::new(TrayAvailability::Registering),
            visible: AtomicBool::new(true),
            left_action: match &options.left_click {
                LeftClick::Action(action) => Some(Arc::clone(action)),
                _ => None,
            },
            handlers: Mutex::new(TrayHandlerMap::new()),
            listeners: Mutex::new(HashMap::new()),
            next_listener_id: AtomicU64::new(0),
            rect: Mutex::new(None),
            event_sender: Mutex::new(None),
        }
    }

    // The event-sender plumbing below is used by the Windows global router;
    // dead on other targets.
    /// Store the bridge task's event sender; the global router on Windows
    /// uses it to hand native events to the right runtime.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn set_event_sender(&self, sender: flume::Sender<BackendSignal>) {
        *self.event_sender.lock() = Some(sender);
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn event_sender(&self) -> flume::Sender<BackendSignal> {
        self.event_sender
            .lock()
            .clone()
            .expect("bridge task installed an event sender")
    }

    /// Deliver a signal to the bridge task; a closed tray drops it.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn send_backend_signal(&self, signal: BackendSignal) {
        if self.is_closed() {
            return;
        }
        if let Some(sender) = self.event_sender.lock().as_ref() {
            let _ = sender.send(signal);
        }
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn availability(&self) -> TrayAvailability {
        self.availability.lock().clone()
    }

    /// Update the availability state; returns the previous value.
    pub(crate) fn set_availability(
        &self,
        availability: TrayAvailability,
    ) -> TrayAvailability {
        let mut guard = self.availability.lock();
        std::mem::replace(&mut guard, availability)
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
    }

    pub(crate) fn replace_handlers(&self, handlers: TrayHandlerMap) {
        *self.handlers.lock() = handlers;
    }

    pub(crate) fn clear_handlers(&self) {
        self.handlers.lock().clear();
    }

    pub(crate) fn add_listener(&self, listener: Listener) -> u64 {
        let id = self
            .next_listener_id
            .fetch_add(1, Ordering::SeqCst);
        self.listeners.lock().insert(id, listener);
        id
    }

    pub(crate) fn remove_listener(&self, id: u64) {
        self.listeners.lock().remove(&id);
    }

    // Written by the Windows event pump; dead on other targets.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn store_rect(&self, rect: Option<TrayRect>) {
        *self.rect.lock() = rect;
    }

    pub(crate) fn rect(&self) -> Option<TrayRect> {
        *self.rect.lock()
    }
}

/// The single state owner behind [`crate::Tray`]: the creation options, the
/// platform backend and its bridge task, and the shared dispatch state.
pub(crate) struct TrayRuntime {
    pub(crate) options: TrayOptions,
    pub(crate) backend: crate::platform::PlatformTrayBackend,
    pub(crate) shared: Arc<RuntimeShared>,
}

impl TrayRuntime {
    /// Shut the tray down: close the shared state (so late events are
    /// dropped), clear the handler table, and let the backend remove the
    /// native icon. Dropping the runtime also cancels the bridge task.
    pub(crate) fn shutdown(&self) {
        self.shared.close();
        self.shared.clear_handlers();
        self.backend.shutdown();
    }
}

/// Deliver an event from the native side: dispatch the configured action,
/// fall back to the item callback, then notify subscribers.
pub(crate) fn route_event(shared: &Arc<RuntimeShared>, app: &AsyncApp, event: TrayEvent) {
    match event {
        TrayEvent::MenuItemActivated { id } => {
            dispatch_menu_item(shared, app, &id);
            notify_listeners(shared, app, TrayEvent::MenuItemActivated { id });
        }
        TrayEvent::Activated { .. } => {
            if let Some(action) = &shared.left_action {
                app.update(|cx| cx.dispatch_action(&**action));
            }
            notify_listeners(shared, app, event);
        }
        TrayEvent::AvailabilityChanged { .. } => {
            // Availability bookkeeping happens where the state changes; this
            // event only reaches subscribers.
            notify_listeners(shared, app, event);
        }
        event => notify_listeners(shared, app, event),
    }
}

fn dispatch_menu_item(shared: &Arc<RuntimeShared>, app: &AsyncApp, id: &TrayItemId) {
    let mut handlers = shared.handlers.lock();
    let Some(handler) = handlers.get_mut(id) else {
        return;
    };
    if let Some(action) = &handler.action {
        let action = Arc::clone(action);
        drop(handlers);
        app.update(|cx| cx.dispatch_action(&*action));
        return;
    }
    if let Some(callback) = handler.on_activate.as_mut() {
        app.update(|cx| callback(cx));
    }
}

/// Send an event to every subscriber. Run on the main thread via `app.update`.
pub(crate) fn notify_listeners(shared: &Arc<RuntimeShared>, app: &AsyncApp, event: TrayEvent) {
    if shared.is_closed() {
        return;
    }
    let mut listeners = shared.listeners.lock();
    for listener in listeners.values_mut() {
        let event = event.clone();
        app.update(|cx| listener(event, cx));
    }
}

/// Convenience for backends that report availability changes from a native
/// thread: store the new state and emit the change event over `tx`.
pub(crate) fn set_availability_and_notify(
    shared: &Arc<RuntimeShared>,
    tx: &flume::Sender<BackendSignal>,
    availability: TrayAvailability,
) {
    let previous = shared.set_availability(availability.clone());
    if previous != availability {
        let _ = tx.send(BackendSignal::Event(TrayEvent::AvailabilityChanged {
            availability,
        }));
    }
}

/// Build a [`TrayRect`] from physical-pixel position and size parts.
// Used by the Windows event pump; dead on other targets.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn make_rect(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale_factor: f32,
) -> TrayRect {
    TrayRect::new(
        TrayPoint::new(x, y, scale_factor),
        TraySize::new(width, height),
    )
}

/// Build a [`TrayPoint`] from physical-pixel coordinates.
pub(crate) fn make_point(x: f64, y: f64, scale_factor: f32) -> TrayPoint {
    TrayPoint::new(x, y, scale_factor)
}

/// Helper used by backends to translate a native error into a [`TrayError`].
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn native_error(error: impl std::fmt::Display) -> crate::error::TrayError {
    crate::error::TrayError::Native(format!("{error}").into())
}
