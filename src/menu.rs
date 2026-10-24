//! Tray menu model: a platform-agnostic description of items, submenus and
//! separators, plus the builder used from application code.
//!
//! The authored model ([`TrayMenu`]) carries both render data and dispatch
//! targets. Before a menu reaches a backend it is resolved into a cloneable,
//! callback-free snapshot ([`ResolvedMenu`]) plus a handler map that only the
//! runtime keeps.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{Action, App, Keystroke, SharedString};

use crate::error::TrayError;
use crate::icon::TrayIcon;

/// A stable identifier for a tray menu item, echoed back in
/// [`crate::TrayEvent::MenuItemActivated`] so the host can map a click to the
/// originating item without comparing labels.
///
/// Ids must be unique within one menu; resolution rejects duplicates. Ids must
/// not contain `:` — the native backends combine the tray id and the item id
/// into a single native identifier using that separator.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TrayItemId {
    id: SharedString,
}

impl TrayItemId {
    /// Create an item id.
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self { id: id.into() }
    }

    /// The identifier as a string.
    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl From<&str> for TrayItemId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for TrayItemId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<SharedString> for TrayItemId {
    fn from(s: SharedString) -> Self {
        Self::new(s)
    }
}

impl std::fmt::Display for TrayItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

/// A boxed, sendable GPUI action stored by the tray menu. `Sync` is required so
/// the action can live behind the shared state and cross the bridge task.
pub type AnyAction = Arc<dyn Action + Send + Sync + 'static>;

/// Dispatch handler kept alive for the lifetime of a rendered menu.
pub(crate) struct Handler {
    pub action: Option<AnyAction>,
    pub on_activate: Option<Box<dyn FnMut(&mut App) + Send>>,
}

/// The dispatch handlers of one resolved menu, keyed by item id.
pub(crate) type TrayHandlerMap = HashMap<TrayItemId, Handler>;

/// An entry in the authored tray menu: a leaf item, a submenu, or a separator.
pub enum TrayEntry {
    /// A horizontal rule between entries.
    Separator,
    /// A nested menu.
    Submenu(TraySubmenu),
    /// A leaf item.
    Item(TrayItem),
}

/// A labeled submenu container.
pub struct TraySubmenu {
    label: SharedString,
    entries: Vec<TrayEntry>,
}

impl TraySubmenu {
    /// Create a submenu from its label and entries.
    pub fn new(label: impl Into<SharedString>, entries: Vec<TrayEntry>) -> Self {
        Self {
            label: label.into(),
            entries,
        }
    }

    /// The label the platform shows for the submenu.
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// The entries the submenu holds, in display order.
    pub fn entries(&self) -> &[TrayEntry] {
        &self.entries
    }
}

/// The authored tray menu.
#[derive(Default)]
pub struct TrayMenu {
    entries: Vec<TrayEntry>,
}

impl TrayMenu {
    /// An empty menu.
    pub fn new() -> Self {
        Self::default()
    }

    /// The entries of the menu, in display order.
    pub fn entries(&self) -> &[TrayEntry] {
        &self.entries
    }
}

/// A menu entry carrying either a GPUI [`Action`] or a plain callback.
///
/// Construct through [`TrayItem::builder`] or
/// [`TrayMenuBuilder::item`].
pub struct TrayItem {
    id: TrayItemId,
    label: SharedString,
    enabled: bool,
    checked: Option<bool>,
    action: Option<AnyAction>,
    on_activate: Option<Box<dyn FnMut(&mut App) + Send>>,
    accelerator: Option<Keystroke>,
    icon: Option<TrayIcon>,
}

impl TrayItem {
    /// Start building an item with a stable id and label.
    pub fn builder(id: impl Into<TrayItemId>, label: impl Into<SharedString>) -> TrayItemBuilder {
        TrayItemBuilder {
            item: Self {
                id: id.into(),
                label: label.into(),
                enabled: true,
                checked: None,
                action: None,
                on_activate: None,
                accelerator: None,
                icon: None,
            },
        }
    }

    /// The identifier echoed back on activation.
    pub fn id(&self) -> &TrayItemId {
        &self.id
    }

    /// The label the platform shows for the item.
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// Whether the item can be activated.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The check state of the item; `Some` renders it as a checkable item.
    pub fn checked(&self) -> Option<bool> {
        self.checked
    }

    /// The shortcut the platform shows next to the label. Display only — this
    /// is not a global hotkey.
    pub fn accelerator(&self) -> Option<&Keystroke> {
        self.accelerator.as_ref()
    }

    /// The icon the platform shows next to the label.
    pub fn icon(&self) -> Option<&TrayIcon> {
        self.icon.as_ref()
    }
}

/// Builder for [`TrayItem`], created by [`TrayItem::builder`].
pub struct TrayItemBuilder {
    item: TrayItem,
}

impl TrayItemBuilder {
    /// Dispatch a GPUI action when the item is activated.
    // `Send + Sync` are required by `AnyAction`; clippy calls the `Send`
    // bound redundant, but removing it breaks the build.
    #[allow(clippy::implied_bounds_in_impls)]
    pub fn with_action(mut self, action: impl Action + Send + Sync + 'static) -> Self {
        self.item.action = Some(Arc::new(action));
        self
    }

    /// Run a callback when the item is activated; used when there is no action.
    pub fn with_callback(mut self, callback: impl FnMut(&mut App) + Send + 'static) -> Self {
        self.item.on_activate = Some(Box::new(callback));
        self
    }

    /// Set whether the item can be activated.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.item.enabled = enabled;
        self
    }

    /// Render the item with a check state.
    pub fn with_checked(mut self, checked: bool) -> Self {
        self.item.checked = Some(checked);
        self
    }

    /// Show a shortcut next to the label.
    pub fn with_accelerator(mut self, accelerator: Keystroke) -> Self {
        self.item.accelerator = Some(accelerator);
        self
    }

    /// Show an icon next to the label.
    pub fn with_icon(mut self, icon: TrayIcon) -> Self {
        self.item.icon = Some(icon);
        self
    }

    /// Finish building.
    pub fn build(self) -> TrayItem {
        self.item
    }
}

/// Builder for [`TrayMenu`], used by [`tray_menu`].
pub struct TrayMenuBuilder {
    menu: TrayMenu,
    /// Shared with nested submenu builders so auto-generated ids stay unique
    /// across the whole menu.
    next_id: Rc<Cell<usize>>,
    /// The item started by [`Self::item`], flushed by the next menu-level call.
    pending_builder: Option<TrayItemBuilder>,
}

impl Default for TrayMenuBuilder {
    fn default() -> Self {
        Self {
            menu: TrayMenu::new(),
            next_id: Rc::new(Cell::new(0)),
            pending_builder: None,
        }
    }
}

impl TrayMenuBuilder {
    fn fresh_id(&mut self) -> TrayItemId {
        let n = self.next_id.get();
        self.next_id.set(n + 1);
        TrayItemId::new(format!("__tray_item_{}", n))
    }

    fn flush_pending(&mut self) {
        if let Some(builder) = self.pending_builder.take() {
            self.menu.entries.push(TrayEntry::Item(builder.item));
        }
    }

    /// Start an item with a stable id. Chain the returned builder's `with_*`
    /// methods; the item is appended when the next menu-level call happens.
    ///
    /// ```ignore
    /// tray_menu(|m| {
    ///     m.item("quit", "Quit").with_action(Quit);
    /// })
    /// ```
    pub fn item(
        &mut self,
        id: impl Into<TrayItemId>,
        label: impl Into<SharedString>,
    ) -> &mut TrayItemBuilder {
        self.flush_pending();
        self.pending_builder = Some(TrayItemBuilder {
            item: TrayItem::builder(id, label).build(),
        });
        self.pending_builder.as_mut().unwrap()
    }

    /// Append an action item with an automatically generated id.
    pub fn action<A: Action + Send + Sync + 'static>(
        &mut self,
        action: A,
        label: impl Into<SharedString>,
    ) -> &mut Self {
        let id = self.fresh_id();
        self.flush_pending();
        self.menu.entries.push(TrayEntry::Item(
            TrayItem::builder(id, label).with_action(action).build(),
        ));
        self
    }

    /// Append a checkable action item with an automatically generated id.
    pub fn check<A: Action + Send + Sync + 'static>(
        &mut self,
        checked: bool,
        action: A,
        label: impl Into<SharedString>,
    ) -> &mut Self {
        let id = self.fresh_id();
        self.flush_pending();
        self.menu.entries.push(TrayEntry::Item(
            TrayItem::builder(id, label)
                .with_action(action)
                .with_checked(checked)
                .build(),
        ));
        self
    }

    /// Append an item that runs a callback instead of an action.
    pub fn callback(
        &mut self,
        label: impl Into<SharedString>,
        on_activate: Box<dyn FnMut(&mut App) + Send>,
    ) -> &mut Self {
        let id = self.fresh_id();
        self.flush_pending();
        self.menu.entries.push(TrayEntry::Item(
            TrayItem::builder(id, label).with_callback(on_activate).build(),
        ));
        self
    }

    /// Append a separator.
    pub fn separator(&mut self) -> &mut Self {
        self.flush_pending();
        self.menu.entries.push(TrayEntry::Separator);
        self
    }

    /// Append a submenu built by the supplied closure.
    pub fn submenu(
        &mut self,
        label: impl Into<SharedString>,
        f: impl FnOnce(&mut TrayMenuBuilder),
    ) -> &mut Self {
        self.flush_pending();
        let mut sub = TrayMenuBuilder {
            menu: TrayMenu::new(),
            // Nested builders share the id counter so auto-generated ids stay
            // unique across the whole menu.
            next_id: Rc::clone(&self.next_id),
            pending_builder: None,
        };
        f(&mut sub);
        self.menu.entries.push(TrayEntry::Submenu(TraySubmenu::new(
            label,
            sub.menu.entries,
        )));
        self
    }

    /// Finish building and return the [`TrayMenu`].
    pub fn build(mut self) -> TrayMenu {
        self.flush_pending();
        self.menu
    }
}

/// Build a [`TrayMenu`] from a closure that populates a [`TrayMenuBuilder`].
///
/// ```ignore
/// use gpui_tray::*;
/// let menu = tray_menu(|m| {
///     m.action(Quit, "Quit");
///     m.separator();
///     m.submenu("More", |m| {
///         m.action(ShowWindow, "Show Window");
///     });
/// });
/// # actions!(demo, [Quit, ShowWindow]);
/// ```
pub fn tray_menu(f: impl FnOnce(&mut TrayMenuBuilder)) -> TrayMenu {
    let mut b = TrayMenuBuilder::default();
    f(&mut b);
    b.build()
}

/// A cloneable, callback-free menu snapshot handed to the backends.
#[derive(Clone)]
pub struct ResolvedMenu {
    pub(crate) entries: Vec<ResolvedEntry>,
}

impl ResolvedMenu {
    /// The resolved entries, in display order.
    pub fn entries(&self) -> &[ResolvedEntry] {
        &self.entries
    }

    /// An empty snapshot.
    pub fn empty() -> Self {
        Self { entries: Vec::new() }
    }
}

/// A resolved entry: leaf item, submenu, or separator.
#[derive(Clone)]
pub enum ResolvedEntry {
    /// A horizontal rule.
    Separator,
    /// A nested menu.
    Submenu(ResolvedSubmenu),
    /// A leaf item.
    Item(ResolvedItem),
}

/// A resolved submenu.
#[derive(Clone)]
pub struct ResolvedSubmenu {
    pub(crate) label: SharedString,
    pub(crate) entries: Vec<ResolvedEntry>,
}

impl ResolvedSubmenu {
    /// The label the platform shows for the submenu.
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// The resolved entries of the submenu.
    pub fn entries(&self) -> &[ResolvedEntry] {
        &self.entries
    }
}

/// A resolved leaf item: render data only, no dispatch targets.
#[derive(Clone)]
pub struct ResolvedItem {
    pub(crate) id: TrayItemId,
    pub(crate) label: SharedString,
    pub(crate) enabled: bool,
    pub(crate) checked: Option<bool>,
    pub(crate) accelerator: Option<Keystroke>,
    pub(crate) icon: Option<TrayIcon>,
}

impl ResolvedItem {
    /// The identifier echoed back on activation.
    pub fn id(&self) -> &TrayItemId {
        &self.id
    }

    /// The label the platform shows for the item.
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// Whether the item can be activated.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The check state; `Some` renders it as a checkable item.
    pub fn checked(&self) -> Option<bool> {
        self.checked
    }

    /// The shortcut displayed next to the label.
    pub fn accelerator(&self) -> Option<&Keystroke> {
        self.accelerator.as_ref()
    }

    /// The icon displayed next to the label.
    pub fn icon(&self) -> Option<&TrayIcon> {
        self.icon.as_ref()
    }
}

/// Resolve an authored menu into a render-safe snapshot plus the dispatch
/// handlers the runtime keeps. Duplicate ids are rejected here, before any
/// native call.
pub(crate) fn resolve_menu(menu: TrayMenu) -> Result<(ResolvedMenu, TrayHandlerMap), TrayError> {
    let mut handlers = TrayHandlerMap::new();
    let entries = resolve_entries(menu.entries, &mut handlers)?;
    Ok((ResolvedMenu { entries }, handlers))
}

fn resolve_entries(
    entries: Vec<TrayEntry>,
    handlers: &mut TrayHandlerMap,
) -> Result<Vec<ResolvedEntry>, TrayError> {
    entries
        .into_iter()
        .map(|entry| match entry {
            TrayEntry::Separator => Ok(ResolvedEntry::Separator),
            TrayEntry::Submenu(sub) => {
                let sub_entries = resolve_entries(sub.entries, handlers)?;
                Ok(ResolvedEntry::Submenu(ResolvedSubmenu {
                    label: sub.label,
                    entries: sub_entries,
                }))
            }
            TrayEntry::Item(item) => {
                let id = item.id.clone();
                if !id.as_str().contains(':') {
                    // Reserved separator used to compose native identifiers.
                }
                if handlers.contains_key(&id) {
                    return Err(TrayError::InvalidMenu(
                        format!("duplicate item id `{id}`").into(),
                    ));
                }
                if id.as_str().contains(':') {
                    return Err(TrayError::InvalidMenu(
                        format!("item id `{id}` must not contain `:`").into(),
                    ));
                }
                handlers.insert(
                    id.clone(),
                    Handler {
                        action: item.action,
                        on_activate: item.on_activate,
                    },
                );
                Ok(ResolvedEntry::Item(ResolvedItem {
                    id,
                    label: item.label,
                    enabled: item.enabled,
                    checked: item.checked,
                    accelerator: item.accelerator,
                    icon: item.icon,
                }))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_moves_callback_out_of_item() {
        let menu = tray_menu(|m| {
            m.callback("cb", Box::new(|_cx: &mut App| {}));
        });
        let (render, handlers) = resolve_menu(menu).unwrap();

        assert_eq!(handlers.len(), 1);

        let ResolvedEntry::Item(item) = &render.entries[0] else {
            panic!("expected a single menu item");
        };
        assert_eq!(item.id.as_str(), "__tray_item_0");
        assert_eq!(handlers.len(), 1);
    }

    #[test]
    fn resolve_rejects_duplicate_ids() {
        let item = || TrayItem::builder("quit", "Quit").with_callback(|_| {}).build();
        let menu = TrayMenu {
            entries: vec![TrayEntry::Item(item()), TrayEntry::Item(item())],
        };
        assert!(resolve_menu(menu).is_err());
    }

    #[test]
    fn resolve_rejects_colon_in_ids() {
        let item = TrayItem::builder("a:b", "Bad").with_callback(|_| {}).build();
        let menu = TrayMenu {
            entries: vec![TrayEntry::Item(item)],
        };
        assert!(resolve_menu(menu).is_err());
    }

    #[test]
    fn resolve_accepts_stable_ids() {
        let quit = TrayItem::builder("quit", "Quit").with_callback(|_| {}).build();
        let menu = TrayMenu {
            entries: vec![TrayEntry::Item(quit)],
        };
        let (render, handlers) = resolve_menu(menu).unwrap();
        assert_eq!(handlers.len(), 1);
        assert_eq!(render.entries.len(), 1);
    }

    gpui::actions!(menu_test, [Quit]);

    #[test]
    fn builder_nests_submenu_items() {
        let menu = tray_menu(|m| {
            m.action(Quit, "Quit");
            m.separator();
            m.submenu("More", |m| {
                m.action(Quit, "Show");
            });
        });
        let (render, handlers) = resolve_menu(menu).unwrap();
        assert_eq!(handlers.len(), 2);
        assert_eq!(render.entries.len(), 3);
    }
}
