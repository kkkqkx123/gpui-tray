//! Manual verification for the `gpui-tray` window lifecycle paths.
//!
//! ```sh
//! cargo run -p tray_lifecycle -- hide-to-tray   # closing the window hides the app; the tray restores it
//! cargo run -p tray_lifecycle -- pure-tray      # no initial window; the tray menu opens one
//! cargo run -p tray_lifecycle -- icon-update    # the tray icon alternates every second
//! ```
//!
//! Each mode opens a window that states what to verify; follow
//! `docs/tests/manual-acceptance-windows.md` for the checklist.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::*;
use gpui_tray::*;

actions!(tray_lifecycle, [ShowWindow, Quit]);

/// Shared state the tray menu and the window both read.
struct LifecycleState {
    main_window: Mutex<Option<AnyWindowHandle>>,
}

impl gpui::Global for LifecycleState {}

struct DemoApp {
    instruction: &'static str,
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
            .child("GPUI Tray Lifecycle")
            .child(self.instruction)
    }
}

/// A solid tray bitmap in one of two colors, so `icon-update` visibly toggles.
fn make_icon(blue: bool) -> Result<TrayIcon, TrayError> {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let (r, g, b) = if blue { (80u8, 160u8, 255u8) } else { (255u8, 140u8, 60u8) };
    for _ in 0..SIZE * SIZE {
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
    TrayIcon::bitmap(SIZE, SIZE, 1, rgba)
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "hide-to-tray".into());

    let instruction: &'static str = match mode.as_str() {
        "hide-to-tray" => "Close this window: the app must hide (not quit) and the tray icon stays. Restore via the tray menu.",
        "pure-tray" => "No window opened at startup. Open one via the tray menu (Show Window).",
        "icon-update" => "The tray icon alternates blue/orange every second; the toggle count prints to stdout.",
        other => {
            eprintln!("unknown mode {other:?}: expected hide-to-tray, pure-tray or icon-update");
            std::process::exit(2);
        }
    };

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.set_global(LifecycleState {
            main_window: Mutex::new(None),
        });

        cx.on_action(|_: &ShowWindow, cx| {
            // Take the handle out of the global first: `global_mut` borrows
            // `cx`, which the window update / open below needs again.
            let handle = cx
                .global_mut::<LifecycleState>()
                .main_window
                .lock()
                .unwrap()
                .clone();
            match handle {
                Some(handle) => {
                    handle
                        .update(cx, |_, window, cx| {
                            window.activate_window();
                            cx.activate(true);
                        })
                        .ok();
                }
                None => {
                    let window = gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                        cx.new(|_| DemoApp {
                            instruction: "Pure-tray mode: this window was opened from the tray menu.",
                        })
                    })
                    .expect("failed to open window");
                    *cx.global_mut::<LifecycleState>()
                        .main_window
                        .lock()
                        .unwrap() = Some(window.0);
                }
            }
        });
        cx.on_action(|_: &Quit, cx| {
            cx.quit();
        });

        let tray = Tray::new(
            cx,
            TrayOptions::builder("tray-lifecycle")
                .with_left_click(LeftClick::Action(Arc::new(ShowWindow)))
                .with_tooltip("GPUI Tray Lifecycle")
                .with_icon(make_icon(true).expect("icon"))
                .build(),
        )
        .expect("failed to create tray");

        tray.set_menu(cx, tray_menu(|m| {
            m.action(ShowWindow, "Show Window");
            m.separator();
            m.action(Quit, "Quit");
        }))
        .expect("failed to set menu");

        println!("Tray available: {}", tray.is_available());

        match mode.as_str() {
            "hide-to-tray" => {
                let window = gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                    cx.new(|_| DemoApp { instruction })
                })
                .expect("failed to open window");
                *cx.global_mut::<LifecycleState>()
                    .main_window
                    .lock()
                    .unwrap() = Some(window.0);

                // Intercept close: returning false keeps the window, and the
                // app hides instead — the tray Show Window entry restores it.
                window
                    .0
                    .update(cx, |_, window, cx| {
                        window.on_window_should_close(cx, |window, cx| {
                            cx.hide();
                            println!("close intercepted: app hidden to tray");
                            let _ = window;
                            false
                        });
                    })
                    .ok();
            }
            "pure-tray" => {
                // Accessory policy removes foreground-app chrome on macOS; on
                // Windows it is accepted as a no-op. No window is opened: the
                // tray menu is the only way in.
                cx.set_activation_policy(gpui::ActivationPolicy::Accessory);
                println!("pure-tray mode: no window; use the tray menu");
            }
            "icon-update" => {
                let window = gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                    cx.new(|_| DemoApp { instruction })
                })
                .expect("failed to open window");
                *cx.global_mut::<LifecycleState>()
                    .main_window
                    .lock()
                    .unwrap() = Some(window.0);

                // Toggle the icon on the main thread once per second.
                let tray = tray.clone();
                cx.spawn(async move |cx| {
                    let mut blue = true;
                    let mut count = 0u32;
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_secs(1))
                            .await;
                        blue = !blue;
                        count += 1;
                        cx.update(|cx| {
                            let _ = tray.set_icon(cx, Some(make_icon(blue).expect("icon")));
                        });
                        println!("tray icon toggled (count {count})");
                    }
                })
                .detach();
            }
            _ => unreachable!("mode validated above"),
        }
    });
}
