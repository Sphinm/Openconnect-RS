#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialog;
mod message;
mod tray;
mod window;

use crate::app::SharedState;
use crate::message::{Command, Event};
use crate::tray::SystemTray;
use crate::window::create_main_window;
use openconnect_core::storage::StoredConfigs;
use std::sync::{mpsc, Arc, Mutex};
use windows::{
    Win32::{
        UI::WindowsAndMessaging::{
            self, DispatchMessageW, GetMessageW, GetWindowLongPtrW, LoadIconW,
            ShowWindow, TranslateMessage, GWLP_USERDATA, HICON, IDI_APPLICATION, SW_SHOW,
        },
    },
};

fn main() {
    // 1. UAC elevation
    #[cfg(target_os = "windows")]
    {
        use openconnect_core::elevator::windows::{elevate, is_elevated};

        if !is_elevated() {
            let exe_path = std::env::current_exe().expect("Failed to get executable path");
            let exe_path = exe_path
                .to_str()
                .expect("Failed to convert path to string");
            let args: Vec<String> = std::env::args().skip(1).collect();
            let mut cmd = std::process::Command::new(exe_path);
            let cmd = cmd.args(&args);

            #[cfg(debug_assertions)]
            const IS_DEBUG: bool = true;
            #[cfg(not(debug_assertions))]
            const IS_DEBUG: bool = false;

            elevate(cmd, IS_DEBUG).expect("Failed to elevate");
            std::process::exit(0);
        }
    }

    // 2. Create channels
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (event_tx, event_rx) = mpsc::channel::<Event>();

    // 3. Get config file path
    let config_file = StoredConfigs::getorinit_config_file().expect("Failed to get config file");

    // 4. Shared state for background thread to post messages
    let shared = Arc::new(SharedState {
        hwnd: Mutex::new(None),
    });

    // 5. Spawn tokio runtime on background thread
    let shared_bg = shared.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async {
            let mut app = app::App::new(cmd_rx, event_tx, config_file, shared_bg)
                .expect("Failed to create app state");
            app.run().await;
        });
    });

    // 6. Load icons (use system default for now, TODO: embed custom icons)
    let icon: HICON = unsafe { LoadIconW(None, IDI_APPLICATION).unwrap() };

    // 7. Create main window
    let hwnd = create_main_window(cmd_tx.clone(), event_rx, icon);

    // Store hwnd in shared state so the background thread can post refresh messages
    {
        let mut guard = shared.hwnd.lock().unwrap();
        *guard = Some(hwnd);
    }

    // 8. Create system tray
    let mut tray = Some(SystemTray::new(hwnd, icon, icon));

    // 9. Show window
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }

    // 10. Enter message loop
    let mut msg = WindowsAndMessaging::MSG::default();

    unsafe {
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == 0 {
                break; // WM_QUIT
            }
            if ret.0 == -1 {
                break; // Error
            }

            // Check for our custom tray notification before dispatching
            if msg.message == crate::tray::WM_TRAY_NOTIFY {
                let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut window::WindowState;

                if !state_ptr.is_null() {
                    let state = &*state_ptr;
                    let current_servers = state.servers.clone();
                    let current_selected = state.selected_server.clone();
                    let is_connected = state.current_status == "CONNECTED";

                    if let Some(ref mut t) = tray {
                        t.set_connected(is_connected);
                    }

                    crate::tray::handle_tray_notify(
                        hwnd,
                        msg.lParam,
                        &mut tray,
                        &current_servers,
                        current_selected.as_deref(),
                        is_connected,
                        &cmd_tx,
                    );
                }
                continue;
            }

            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // Cleanup: disconnect if needed
    let _ = cmd_tx.send(Command::Disconnect);
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(tray);
}
