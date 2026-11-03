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

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::*;
use gpui_tray::*;

actions!(tray_lifecycle, [ShowWindow, Quit]);

// Placement self-check (see docs/tests/manual-acceptance-windows.md):
// Windows 11's notification area can accept `NIM_ADD` (`NIM_ADD` returns
// TRUE) yet not actually place the icon — most often after earlier runs
// were killed without `NIM_DELETE`, or while the shell's tray subsystem is
// still starting up. `Shell_NotifyIconGetRect` is the objective placement
// probe, so the first frames print availability + rect.
thread_local! {
    static DIAG_TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    static DIAG_FRAMES: Cell<u32> = const { Cell::new(0) };
}
/// Windows-only helpers: gpui has no per-window hide (and `App::hide` is a
/// no-op on this platform), so the demo hides/restores the HWND directly via
/// raw-window-handle + `ShowWindow`.
#[cfg(target_os = "windows")]
mod win {
    use gpui_kit::Window;
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, PostMessageW, ShowWindow, WM_CLOSE, SW_HIDE, SW_SHOW,
    };

    pub fn hwnd_of(window: &Window) -> Option<HWND> {
        // Fully qualified: `gpui::Window` has an inherent `window_handle()`
        // that returns its `AnyWindowHandle`, shadowing the trait method.
        let handle = raw_window_handle::HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win32) => {
                Some(HWND(win32.hwnd.get() as *mut core::ffi::c_void))
            }
            _ => None,
        }
    }

    /// `IsWindowVisible` for the window's HWND; `None` when the HWND cannot
    /// be resolved through raw-window-handle.
    pub fn visibility(window: &Window) -> Option<bool> {
        let hwnd = hwnd_of(window)?;
        Some(unsafe { IsWindowVisible(hwnd) }.as_bool())
    }

    pub fn hide(window: &Window) {
        match hwnd_of(window) {
            Some(hwnd) => {
                let before = unsafe { IsWindowVisible(hwnd) }.as_bool();
                let previous = unsafe { ShowWindow(hwnd, SW_HIDE) }.as_bool();
                let after = unsafe { IsWindowVisible(hwnd) }.as_bool();
                println!(
                    "win::hide: hwnd={hwnd:?} visible {before} -> {after} (ShowWindow previous={previous})"
                );
            }
            None => println!("win::hide: no HWND resolved via raw-window-handle"),
        }
    }

    pub fn show(window: &Window) {
        match hwnd_of(window) {
            Some(hwnd) => {
                let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
                let now = unsafe { IsWindowVisible(hwnd) }.as_bool();
                println!("win::show: hwnd={hwnd:?} now visible = {now}");
            }
            None => println!("win::show: no HWND resolved via raw-window-handle"),
        }
    }

    /// Ask the window to close through the standard path: the registered
    /// `on_window_should_close` interceptor decides what happens.
    pub fn post_close(window: &Window) {
        match hwnd_of(window) {
            Some(hwnd) => unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            None => println!("win::post_close: no HWND resolved via raw-window-handle"),
        }
    }
}

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
        DIAG_FRAMES.with(|f| f.set(f.get() + 1));
        let frame = DIAG_FRAMES.with(Cell::get);
        if frame == 1 || frame == 90 {
            DIAG_TRAY.with(|t| {
                if let Some(tray) = t.borrow().as_ref() {
                    println!(
                        "diagnostic frame {frame}: availability={:?} rect={:?}",
                        tray.availability(),
                        tray.rect()
                    );
                }
            });
        }
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
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let auto_close = argv.iter().any(|a| a == "--auto-close");
    let mode = argv
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
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
            /// Open a fresh main window and remember its handle.
            fn open_main_window(cx: &mut gpui_kit::App) {
                let window = gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                    cx.new(|_| DemoApp {
                        instruction: "This window was opened from the tray menu.",
                    })
                })
                .expect("failed to open window");
                *cx.global_mut::<LifecycleState>()
                    .main_window
                    .lock()
                    .unwrap() = Some(window.0);
            }

            match handle {
                Some(handle) => {
                    let alive = handle
                        .update(cx, |_, window, cx| {
                            #[cfg(target_os = "windows")]
                            win::show(window);
                            window.activate_window();
                            cx.activate(true);
                        })
                        .is_ok();
                    if !alive {
                        // The remembered window was destroyed (closing the last
                        // window does not quit the process on Windows): open a
                        // fresh one instead of failing silently.
                        open_main_window(cx);
                    }
                }
                None => open_main_window(cx),
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
        DIAG_TRAY.set(Some(tray.clone()));

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
                            // `App::hide` is a no-op on Windows (gpui-pre-
                            // windows 0.3.10 `fn hide(&self) {}`), so hide the
                            // HWND there directly; elsewhere the app-level
                            // hide still applies.
                            #[cfg(target_os = "windows")]
                            {
                                win::hide(window);
                                let _ = cx;
                            }
                            #[cfg(not(target_os = "windows"))]
                            cx.hide();
                            println!("close intercepted: window hidden to tray");
                            false
                        });
                    })
                    .ok();

                // Programmatic close: post `WM_CLOSE` ourselves so the whole
                // intercept + hide path reports to stdout without a human
                // click, then quit. Windows-only diagnostic.
                #[cfg(target_os = "windows")]
                if auto_close {
                    let handle = window.0;
                    cx.spawn(async move |cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(600))
                            .await;
                        let _ = cx.update(|cx| {
                            let _ = handle.update(cx, |_, window, _cx| {
                                println!(
                                    "auto-close: visibility before WM_CLOSE = {:?}",
                                    win::visibility(window)
                                );
                                win::post_close(window);
                            });
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(1500))
                            .await;
                        let _ = cx.update(|cx| {
                            let _ = handle.update(cx, |_, window, _cx| {
                                println!(
                                    "auto-close: visibility after = {:?}",
                                    win::visibility(window)
                                );
                            });
                        });
                        let _ = cx.update(|cx| cx.quit());
                    })
                    .detach();
                }
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
