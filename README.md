[English](README.md) | 简体中文

# gpui-tray

System tray / menu-bar icon support for
[GPUI](https://github.com/zed-industries/zed) applications.

- One API for macOS, Windows and Linux; every operation takes the selected
  GPUI flavor's `gpui::App`.
- Menu items carry a GPUI `Action` (dispatched through the normal action
  pipeline) or a plain callback, so a tray menu behaves like the application
  menu and the keybinding paths already in GPUI.
- Native menus on every platform: `DBusMenu` on Linux, `NSMenu` on macOS and
  `HMENU` on Windows.
- The tray never blocks startup. When the platform has no usable tray (GNOME
  without the AppIndicator extension, for instance), `Tray::is_available()`
  returns `false` and the application runs normally.

## Usage

```toml
[dependencies]
gpui-kit = "0.8"
gpui-tray = { version = "0.1", default-features = false, features = ["gpui-kit"] }
```

```rust
use gpui_kit::*;
use gpui_tray::*;

actions!(demo, [ShowWindow, Quit]);

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);

        let tray = Tray::new(
            cx,
            TrayOptions::builder("demo.tray")
                .with_left_click(LeftClick::Action(std::sync::Arc::new(ShowWindow)))
                .with_tooltip("Demo")
                .build(),
        )
        .expect("failed to create tray");
        tray.set_menu(cx, tray_menu(|m| {
            m.action(ShowWindow, "Show Window");
            m.separator();
            m.action(Quit, "Quit");
        }))
        .expect("failed to set menu");
    });
}
```

`Tray::new` must run on the GPUI thread, inside
`application().run(|cx| ..)`, like every other handle into application state.
The tray is `!Send`: the type enforces the main-thread constraint of the
native handles behind it. The left-click behavior is fixed at creation time
through `TrayOptions` — the Linux backend bakes it into the StatusNotifierItem
when it registers.

## GPUI flavors

Select exactly one flavor, and select the one the application itself builds
against — the tray takes that flavor's `gpui::App` and dispatches that
flavor's `Action`.

| Feature     | GPUI it binds to                    |
| ----------- | ----------------------------------- |
| `gpui-kit`  | GPUI Kit's snapshot (`gpui-pre`)    |
| `gpui-fast` | GPUI Fast (`gpui-fast`)             |

`gpui-kit` is the default. Choosing the other one means disabling the default
features:

```toml
gpui-tray = { version = "0.1", default-features = false, features = ["gpui-fast"] }
```

## Platform notes

| Platform | Backend                        | Left click                    | Menu           |
| -------- | ------------------------------ | ----------------------------- | -------------- |
| Linux    | `ksni` (StatusNotifierItem)    | menu / action, fixed at creation (`TrayOptions`) | `DBusMenu`     |
| macOS    | `objc2` `NSStatusItem`         | menu / action, fixed at creation (`TrayOptions`) | `NSMenu`        |
| Windows  | `tray-icon` (`Shell_NotifyIcon`) | menu / action, fixed at creation (`TrayOptions`) | `HMENU`         |

On Linux the desktop shell draws the menu, so the tray icon only appears when
the session provides a StatusNotifierItem host: KDE Plasma does, GNOME needs
the AppIndicator extension.

## Example

`examples/tray_demo` shows a tray icon with a native menu, a generated icon, a
tooltip and left-click wiring:

```bash
cargo run --example tray_demo
```

## License

Apache-2.0
