//! Tray demo: shows a system tray / menu-bar icon with a native menu, a generated
//! icon, a tooltip, and left-click action wiring. Run with `cargo run
//! --example tray_demo` on a desktop session that supports a tray (KDE/GNOME+
//! AppIndicator on Linux, the menu bar on macOS, the taskbar on Windows).

use std::sync::Arc;

use gpui_kit::*;
use gpui_tray::*;

actions!(tray_demo, [ShowWindow, ToggleAutoStart, Quit]);

/// Shared demo state the tray menu and the window both read.
#[derive(Default)]
struct DemoState {
    auto_start: bool,
    main_window: std::sync::Mutex<Option<AnyWindowHandle>>,
}

impl gpui::Global for DemoState {}

struct DemoApp;

impl Render for DemoApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = cx.global::<DemoState>();
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child("GPUI Tray Demo")
            .child(format!("Launch at login: {}", state.auto_start))
    }
}

fn make_icon() -> Result<TrayIcon, TrayError> {
    let size = 32u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let r = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - r;
            let dy = y as f32 - r;
            let inside = (dx * dx + dy * dy) <= r * r;
            let (cr, cg, cb, ca) = if inside {
                (80u8, 160u8, 255u8, 255u8)
            } else {
                (0, 0, 0, 0)
            };
            rgba.extend_from_slice(&[cr, cg, cb, ca]);
        }
    }
    TrayIcon::bitmap(size, size, 1, rgba)
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        cx.set_global(DemoState::default());

        // Action handlers: the tray menu dispatches through the normal GPUI
        // action pipeline, so these serve tray clicks and keybindings alike.
        cx.on_action(|_: &ShowWindow, cx| {
            let handle = cx.global::<DemoState>().main_window.lock().unwrap().clone();
            if let Some(handle) = handle {
                handle
                    .update(cx, |_, window, cx| {
                        window.activate_window();
                        cx.activate(true);
                    })
                    .ok();
            }
        });
        cx.on_action(|_: &ToggleAutoStart, cx| {
            let state = cx.global_mut::<DemoState>();
            state.auto_start = !state.auto_start;
        });
        cx.on_action(|_: &Quit, cx| {
            cx.quit();
        });

        let icon = make_icon().expect("icon");

        // Left click restores the window; the menu is also available via right
        // click.
        let tray = Tray::new(
            cx,
            TrayOptions::builder("tray-demo")
                .with_left_click(LeftClick::Action(Arc::new(ShowWindow)))
                .with_tooltip("GPUI Tray Demo")
                .with_icon(icon)
                .build(),
        )
        .expect("failed to create tray");

        tray.set_menu(cx, tray_menu(|m| {
            m.action(ShowWindow, "Show Window");
            m.separator();
            m.check(false, ToggleAutoStart, "Launch at Login");
            m.separator();
            m.action(Quit, "Quit");
        }))
        .expect("failed to set menu");

        let window = gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| DemoApp)
        })
        .expect("failed to open window");

        cx.global_mut::<DemoState>().main_window =
            std::sync::Mutex::new(Some(window.0));

        println!("Tray available: {}", tray.is_available());
    });
}
