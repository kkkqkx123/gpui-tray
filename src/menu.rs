//! Tray menu model: a platform-agnostic description of items, submenus and
//! separators, plus the builder used from application code.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Action, App, Keystroke, SharedString};

use crate::icon::TrayIconRef;

/// Stable identifier for a tray menu item, echoed back in [`TrayEvent`]s so the
/// host can map a click to the originating item without comparing labels.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MenuItemId(
    /// The identifier itself.
    pub SharedString,
);

impl MenuItemId {
    /// Build an identifier from anything that converts into a [`SharedString`].
    pub fn new(s: impl Into<SharedString>) -> Self {
        MenuItemId(s.into())
    }
}

impl From<&str> for MenuItemId {
    fn from(s: &str) -> Self {
        MenuItemId(s.into())
    }
}

impl From<String> for MenuItemId {
    fn from(s: String) -> Self {
        MenuItemId(s.into())
    }
}

impl From<SharedString> for MenuItemId {
    fn from(s: SharedString) -> Self {
        MenuItemId(s)
    }
}

/// A boxed, sendable GPUI action stored by the tray menu. `Sync` is required so
/// the action can live behind the shared `Mutex` and cross the bridge task.
pub type AnyAction = Arc<dyn Action + Send + Sync + 'static>;

/// Dispatch handler kept alive for the lifetime of a rendered menu.
pub(crate) struct Handler {
    pub action: Option<AnyAction>,
    pub on_activate: Option<Box<dyn FnMut(&mut App) + Send>>,
}

/// A single menu item, a submenu, or a separator.
///
/// Not `Clone`: [`TrayItem`] may carry a `Box<dyn FnMut>` callback that cannot be
/// cloned. Backends consume menus by value (via [`split_menu`]), never by clone.
pub enum TrayMenuItem {
    /// A horizontal rule between items.
    Separator,
    /// A nested menu.
    Submenu(TraySubmenu),
    /// A leaf item.
    Item(TrayItem),
}

/// A labeled submenu container.
pub struct TraySubmenu {
    /// The label the platform shows for the submenu.
    pub label: SharedString,
    /// The items the submenu holds.
    pub items: Vec<TrayMenuItem>,
}

/// The tray menu model.
#[derive(Default)]
pub struct TrayMenu {
    /// The items the menu holds, in display order.
    pub items: Vec<TrayMenuItem>,
}

/// A menu entry carrying either a GPUI [`Action`] or a plain callback.
pub struct TrayItem {
    /// The identifier echoed back when the item is activated.
    pub id: MenuItemId,
    /// The label the platform shows for the item.
    pub label: SharedString,
    /// Whether the item can be activated.
    pub enabled: bool,
    /// The check state of the item; `Some` renders it as a checkable item.
    pub checked: Option<bool>,
    /// The action dispatched when the item is activated.
    pub action: Option<AnyAction>,
    /// The callback run when the item is activated; used when there is no
    /// action.
    pub on_activate: Option<Box<dyn FnMut(&mut App) + Send>>,
    /// The shortcut the platform shows next to the label; macOS renders it.
    pub accelerator: Option<Keystroke>,
    /// The icon the platform shows next to the label.
    pub icon: Option<TrayIconRef>,
}

/// Builder for [`TrayMenu`], used by [`tray_menu`].
pub struct TrayMenuBuilder {
    menu: TrayMenu,
    next_id: usize,
}

impl Default for TrayMenuBuilder {
    fn default() -> Self {
        Self {
            menu: TrayMenu { items: Vec::new() },
            next_id: 0,
        }
    }
}

impl TrayMenuBuilder {
    fn fresh_id(&mut self) -> MenuItemId {
        let id = MenuItemId(SharedString::from(format!("__tray_item_{}", self.next_id)));
        self.next_id += 1;
        id
    }

    fn push_item(&mut self, item: TrayItem) -> &mut Self {
        self.menu.items.push(TrayMenuItem::Item(item));
        self
    }

    /// Append an action item.
    pub fn action<A: Action + Send + Sync + 'static>(
        &mut self,
        action: A,
        label: impl Into<SharedString>,
    ) -> &mut Self {
        let id = self.fresh_id();
        self.push_item(TrayItem {
            id,
            label: label.into(),
            enabled: true,
            checked: None,
            action: Some(Arc::new(action)),
            on_activate: None,
            accelerator: None,
            icon: None,
        })
    }

    /// Append a checkable action item.
    pub fn check<A: Action + Send + Sync + 'static>(
        &mut self,
        checked: bool,
        action: A,
        label: impl Into<SharedString>,
    ) -> &mut Self {
        let id = self.fresh_id();
        self.push_item(TrayItem {
            id,
            label: label.into(),
            enabled: true,
            checked: Some(checked),
            action: Some(Arc::new(action)),
            on_activate: None,
            accelerator: None,
            icon: None,
        })
    }

    /// Append an item that runs a callback instead of an action.
    pub fn callback(
        &mut self,
        label: impl Into<SharedString>,
        on_activate: Box<dyn FnMut(&mut App) + Send>,
    ) -> &mut Self {
        let id = self.fresh_id();
        self.push_item(TrayItem {
            id,
            label: label.into(),
            enabled: true,
            checked: None,
            action: None,
            on_activate: Some(on_activate),
            accelerator: None,
            icon: None,
        })
    }

    /// Append a separator.
    pub fn separator(&mut self) -> &mut Self {
        self.menu.items.push(TrayMenuItem::Separator);
        self
    }

    /// Append a submenu built by the supplied closure.
    pub fn submenu(
        &mut self,
        label: impl Into<SharedString>,
        f: impl FnOnce(&mut TrayMenuBuilder),
    ) -> &mut Self {
        let mut sub = TrayMenuBuilder::default();
        f(&mut sub);
        self.menu.items.push(TrayMenuItem::Submenu(TraySubmenu {
            label: label.into(),
            items: sub.menu.items,
        }));
        self
    }

    /// Finish building and return the [`TrayMenu`].
    pub fn build(self) -> TrayMenu {
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

/// Split a [`TrayMenu`] into a render-safe copy (callbacks removed) and a map of
/// dispatch handlers keyed by [`MenuItemId`]. The callbacks are moved out of the
/// items so each backend can rebind them per render without cloning `FnMut`.
pub(crate) fn split_menu(menu: TrayMenu) -> (TrayMenu, HashMap<MenuItemId, Handler>) {
    let mut handlers: HashMap<MenuItemId, Handler> = HashMap::new();
    let items = menu
        .items
        .into_iter()
        .map(|item| match item {
            TrayMenuItem::Item(mut it) => {
                let id = it.id.clone();
                let on_activate = it.on_activate.take();
                handlers.insert(
                    id.clone(),
                    Handler {
                        action: it.action.clone(),
                        on_activate,
                    },
                );
                TrayMenuItem::Item(it)
            }
            TrayMenuItem::Submenu(sub) => {
                let (sub_menu, sub_handlers) = split_menu(TrayMenu { items: sub.items });
                handlers.extend(sub_handlers);
                TrayMenuItem::Submenu(TraySubmenu {
                    label: sub.label,
                    items: sub_menu.items,
                })
            }
            other => other,
        })
        .collect();
    (TrayMenu { items }, handlers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_menu_moves_callback_out_of_item() {
        let menu = tray_menu(|m| {
            m.callback("cb", Box::new(|_cx: &mut App| {}));
        });
        let (render, handlers) = split_menu(menu);

        assert_eq!(handlers.len(), 1);

        let id = match render.items.first() {
            Some(TrayMenuItem::Item(it)) => {
                assert!(
                    it.on_activate.is_none(),
                    "callback must be moved out of the rendered item"
                );
                it.id.clone()
            }
            _ => panic!("expected a single menu item"),
        };

        assert!(
            handlers.get(&id).unwrap().on_activate.is_some(),
            "handler should hold the moved callback"
        );
    }

    #[test]
    fn builder_nests_submenu_items() {
        let menu = tray_menu(|m| {
            m.separator();
            m.submenu("More", |m| {
                m.separator();
            });
        });
        assert_eq!(menu.items.len(), 2);
        match &menu.items[1] {
            TrayMenuItem::Submenu(sub) => assert_eq!(sub.items.len(), 1),
            _ => panic!("expected a submenu at index 1"),
        }
    }
}

