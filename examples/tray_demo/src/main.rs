//! Tray demo: shows a system tray / menu-bar icon with a native menu, a generated
//! icon, a tooltip, and left-click action wiring. Run with `cargo run
//! --example tray_demo` on a desktop session that supports a tray (KDE/GNOME+
//! AppIndicator on Linux, the menu bar on macOS, the taskbar on Windows).

use std::sync::Arc;

use gpui_kit::*;
use gpui_tray::*;

actions!(tray_demo, [ShowWindow, ToggleAutoStart, Quit]);

struct DemoApp {
    auto_start: bool,
}

impl Render for DemoApp {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child("GPUI Tray Demo")
            .child(format!("Launch at login: {}", self.auto_start))
    }
}

fn make_icon() -> TrayIconData {
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
    TrayIconData::new(size, size, rgba)
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);

        let tray = Tray::new(cx).expect("failed to create tray");
        tray.set_tooltip(Some("GPUI Tray Demo".into()));
        tray.set_icon(Some(make_icon()));

        // Left click restores the window; the menu is also available via right click.
        tray.set_left_click_action(LeftClick::Action(Arc::new(ShowWindow)));

        tray.set_menu(tray_menu(|m| {
            m.action(ShowWindow, "Show Window");
            m.separator();
            m.check(false, ToggleAutoStart, "Launch at Login");
            m.separator();
            m.action(Quit, "Quit");
        }));

        let _window = gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| DemoApp { auto_start: false })
        })
        .expect("failed to open window");

        println!("Tray available: {}", tray.is_available());
    });
}
