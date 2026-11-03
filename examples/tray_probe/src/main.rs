//! Minimal tray-icon probe: bypasses gpui-tray entirely to isolate why the
//! lifecycle demo's icon does not appear on this Windows 11 box.
//!
//! ```sh
//! cargo run -p tray_probe               # plain registration, 30s pump
//! cargo run -p tray_probe -- 10         # 10s variant
//! cargo run -p tray_probe -- 30 --renotify
//! cargo run -p tray_probe -- 15 --full          # all gpui-tray builder suspects
//! cargo run -p tray_probe -- 15 --menu         # only the muda menu attach
//! ```
//!
//! Every few seconds the probe reads `TrayIcon::rect()` (a
//! `Shell_NotifyIconGetRect` query): `None` means the shell accepted
//! `NIM_ADD` but has not actually placed the icon anywhere — the exact
//! "registered but invisible" state observed with gpui-tray. `--renotify`
//! then hides and re-shows the icon (`NIM_MODIFY`), the standard workaround
//! for a swallowed first `NIM_ADD`.

use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
mod probe {
    use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, PM_REMOVE, TranslateMessage, MSG,
    };

    use super::{Duration, Instant};

    pub fn run(seconds: u64, renotify: bool, menu: bool, id: bool, left_off: bool, template: bool) {
        TrayIconEvent::set_event_handler(Some(|event| {
            println!("probe event: {event:?}");
        }));
        if menu {
            tray_icon::menu::MenuEvent::set_event_handler(Some(|event| {
                println!("probe menu event: {event:?}");
            }));
        }

        // 32x32 solid blue, opaque — the same pixel recipe the lifecycle demo
        // uses.
        let mut rgba = Vec::with_capacity(32 * 32 * 4);
        for _ in 0..32 * 32 {
            rgba.extend_from_slice(&[80u8, 160, 255, 255]);
        }
        let icon = Icon::from_rgba(rgba, 32, 32).expect("icon");

        let mut builder = TrayIconBuilder::new().with_icon(icon).with_tooltip("gpui-tray probe");
        // Suspect flags mirroring gpui-tray's create_backend builder chain.
        if id {
            builder = builder.with_id("probe.tray");
        }
        if menu {
            builder = builder.with_menu(Box::new(tray_icon::menu::Menu::new()));
        }
        if left_off {
            builder = builder.with_menu_on_left_click(false);
        }
        if template {
            builder = builder.with_icon_as_template(false);
        }
        let tray = builder
            .build()
            .expect("probe: Shell_NotifyIconW(NIM_ADD) failed");

        println!("probe: NIM_ADD returned success; check the tray (expand ^) for 'gpui-tray probe'.");
        println!("probe: pumping messages for {seconds}s; printing TrayIcon::rect() every 3s.");

        let start = Instant::now();
        let deadline = Duration::from_secs(seconds);
        let mut next_report = Duration::from_secs(3);
        let mut renotified = false;
        let mut msg = MSG::default();
        unsafe {
            loop {
                let now = start.elapsed();
                if now >= deadline {
                    break;
                }
                if now >= next_report {
                    next_report += Duration::from_secs(3);
                    match tray.rect() {
                        Some(rect) => println!("probe t={now:?} rect=Some({rect:?})"),
                        None => println!("probe t={now:?} rect=None (shell has not placed the icon)"),
                    }
                    if renotify && !renotified {
                        println!("probe: renotify (set_visible false -> true)");
                        let _ = tray.set_visible(false);
                        let _ = tray.set_visible(true);
                        renotified = true;
                    }
                }
                // Pump this thread's messages so the hidden tray window stays
                // responsive — this is the half gpui-tray relies on gpui for.
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        // `tray` drops here: `TrayIcon::drop` sends `NIM_DELETE`, so the
        // probe never leaves a dead registration behind.
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let seconds = args.next().and_then(|v| v.parse().ok()).unwrap_or(30u64);
    let flags: Vec<String> = args.collect();
    let has = |f: &str| flags.iter().any(|a| a == f);
    let renotify = has("--renotify");
    let menu = has("--menu");
    let id = has("--id");
    let left_off = has("--left-off");
    let template = has("--template");
    let (menu, id, left_off, template) = if has("--full") {
        (true, true, true, true)
    } else {
        (menu, id, left_off, template)
    };

    #[cfg(target_os = "windows")]
    probe::run(seconds, renotify, menu, id, left_off, template);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (seconds, renotify, menu, id, left_off, template);
        println!("tray_probe is Windows-only");
    }
    println!("probe: done (icon unregistered).");
}
