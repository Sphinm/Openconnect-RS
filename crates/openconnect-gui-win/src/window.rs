use crate::message::{Command, Event};
use openconnect_core::storage::StoredServer;
use std::sync::{
    mpsc::{Receiver, Sender},
    Mutex,
};
use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::GetStockObject,
        UI::{
            Controls::{self, CBN_SELCHANGE},
            WindowsAndMessaging::{
                self, CreateWindowExW, DefWindowProcW, EnableWindow, GetDlgItem, GetWindowLongPtrW,
                LoadCursorW, SendMessageW, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
                GWLP_USERDATA, IDC_ARROW, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_USER,
                WS_BORDER, WS_CHILD, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
};

// Control IDs
const IDC_SERVER_COMBO: i32 = 1001;
const IDC_MANAGE_BTN: i32 = 1002;
const IDC_CONNECT_BTN: i32 = 1003;
const IDC_SERVER_TYPE_LABEL: i32 = 1004;
const IDC_SERVER_URL_LABEL: i32 = 1005;
const IDC_SERVER_EXTRA_LABEL: i32 = 1006;
const IDC_STATUS_LABEL: i32 = 1007;
const IDC_PANEL_DISCONNECTED: i32 = 1008;
const IDC_PANEL_CONNECTING: i32 = 1009;
const IDC_PANEL_CONNECTED: i32 = 1010;
const IDC_PANEL_ERROR: i32 = 1011;
const IDC_PANEL_CONNECTING_TEXT: i32 = 1012;
const IDC_PANEL_CONNECTED_TEXT: i32 = 1013;
const IDC_PANEL_ERROR_TEXT: i32 = 1014;

const WM_REFRESH_UI: u32 = WM_USER + 1;

pub const WINDOW_WIDTH: i32 = 700;
pub const WINDOW_HEIGHT: i32 = 500;

/// State stored alongside each window instance.
pub struct WindowState {
    pub hwnd: HWND,
    pub cmd_tx: Sender<Command>,
    pub event_rx: Mutex<Receiver<Event>>,
    pub servers: Vec<StoredServer>,
    pub selected_server: Option<String>,
    pub current_status: String,
}

/// Build an HSTRING from a format string.
macro_rules! hformat {
    ($($arg:tt)*) => {{
        let s = format!($($arg)*);
        windows::core::HSTRING::from(s)
    }};
}

impl WindowState {
    fn refresh_ui(&self) {
        let is_idle = matches!(
            self.current_status.as_str(),
            "INITIALIZED" | "DISCONNECTED" | "ERROR"
        );
        let is_connecting = self.current_status == "CONNECTING"
            || self.current_status == "DISCONNECTING";
        let is_connected = self.current_status == "CONNECTED";

        unsafe {
            ShowWindow(GetDlgItem(self.hwnd, IDC_PANEL_DISCONNECTED), if is_idle { SW_SHOW } else { SW_HIDE });
            ShowWindow(GetDlgItem(self.hwnd, IDC_PANEL_CONNECTING), if is_connecting { SW_SHOW } else { SW_HIDE });
            ShowWindow(GetDlgItem(self.hwnd, IDC_PANEL_CONNECTED), if is_connected { SW_SHOW } else { SW_HIDE });
            ShowWindow(
                GetDlgItem(self.hwnd, IDC_PANEL_ERROR),
                if self.current_status == "ERROR" { SW_SHOW } else { SW_HIDE },
            );

            let btn = GetDlgItem(self.hwnd, IDC_CONNECT_BTN);
            if is_connected {
                let _ = SetWindowTextW(btn, w!("Disconnect"));
                let _ = EnableWindow(btn, true);
            } else if is_idle {
                let _ = SetWindowTextW(btn, w!("Connect"));
                let _ = EnableWindow(btn, self.selected_server.is_some());
            } else {
                let _ = EnableWindow(btn, false);
            }
        }
    }

    fn update_server_combo(&self) {
        unsafe {
            let combo = GetDlgItem(self.hwnd, IDC_SERVER_COMBO);
            let _ = SendMessageW(combo, Controls::CB_RESETCONTENT, None, None);

            for server in &self.servers {
                let name = match server {
                    StoredServer::Oidc(s) => &s.name,
                    StoredServer::Password(s) => &s.name,
                };
                let name_wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = SendMessageW(
                    combo,
                    Controls::CB_ADDSTRING,
                    None,
                    Some(LPARAM(name_wide.as_ptr() as isize)),
                );
            }

            if let Some(ref selected) = self.selected_server {
                if let Some(idx) = self.servers.iter().position(|s| match s {
                    StoredServer::Oidc(s) => s.name == *selected,
                    StoredServer::Password(s) => s.name == *selected,
                }) {
                    let _ = SendMessageW(combo, Controls::CB_SETCURSEL, Some(WPARAM(idx)), None);
                }
            } else if !self.servers.is_empty() {
                let _ = SendMessageW(combo, Controls::CB_SETCURSEL, Some(WPARAM(0)), None);
            }
        }
    }

    fn update_server_info(&self) {
        let server = self.selected_server.as_ref().and_then(|name| {
            self.servers.iter().find(|s| match s {
                StoredServer::Oidc(s) => s.name == *name,
                StoredServer::Password(s) => s.name == *name,
            })
        });

        unsafe {
            let type_label = GetDlgItem(self.hwnd, IDC_SERVER_TYPE_LABEL);
            let url_label = GetDlgItem(self.hwnd, IDC_SERVER_URL_LABEL);
            let extra_label = GetDlgItem(self.hwnd, IDC_SERVER_EXTRA_LABEL);

            match server {
                Some(StoredServer::Password(s)) => {
                    let _ = SetWindowTextW(type_label, w!("Type: Password"));
                    let url = hformat!("Server: {}", s.server);
                    let extra = hformat!("Username: {}", s.username);
                    let _ = SetWindowTextW(url_label, &url);
                    let _ = SetWindowTextW(extra_label, &extra);
                }
                Some(StoredServer::Oidc(s)) => {
                    let _ = SetWindowTextW(type_label, w!("Type: OIDC"));
                    let url = hformat!("Server: {}", s.server);
                    let extra = hformat!("Issuer: {}", s.issuer);
                    let _ = SetWindowTextW(url_label, &url);
                    let _ = SetWindowTextW(extra_label, &extra);
                }
                None => {
                    let _ = SetWindowTextW(type_label, w!(""));
                    let _ = SetWindowTextW(url_label, w!(""));
                    let _ = SetWindowTextW(extra_label, w!(""));
                }
            }
        }
    }

    fn set_status_text(&self, status: &str, message: Option<&str>) {
        unsafe {
            let label = GetDlgItem(self.hwnd, IDC_STATUS_LABEL);
            let text: HSTRING = match message {
                Some(msg) => format!("{}: {}", status, msg).into(),
                None => status.into(),
            };
            let _ = SetWindowTextW(label, &text);
        }
    }

    fn set_connecting_text(&self, text: &str) {
        unsafe {
            let label = GetDlgItem(self.hwnd, IDC_PANEL_CONNECTING_TEXT);
            let t: HSTRING = text.into();
            let _ = SetWindowTextW(label, &t);
        }
    }

    fn set_connected_text(&self, text: &str) {
        unsafe {
            let label = GetDlgItem(self.hwnd, IDC_PANEL_CONNECTED_TEXT);
            let t: HSTRING = text.into();
            let _ = SetWindowTextW(label, &t);
        }
    }

    fn set_error_text(&self, text: &str) {
        unsafe {
            let label = GetDlgItem(self.hwnd, IDC_PANEL_ERROR_TEXT);
            let t: HSTRING = text.into();
            let _ = SetWindowTextW(label, &t);
        }
    }

    fn handle_combo_change(&mut self) {
        unsafe {
            let combo = GetDlgItem(self.hwnd, IDC_SERVER_COMBO);
            let idx = SendMessageW(combo, Controls::CB_GETCURSEL, None, None).0;
            if idx >= 0 && (idx as usize) < self.servers.len() {
                let name = match &self.servers[idx as usize] {
                    StoredServer::Oidc(s) => s.name.clone(),
                    StoredServer::Password(s) => s.name.clone(),
                };
                self.selected_server = Some(name);
                self.update_server_info();
                self.refresh_ui();
            }
        }
    }

    fn handle_connect(&mut self) {
        if self.current_status == "CONNECTED" {
            let _ = self.cmd_tx.send(Command::Disconnect);
            return;
        }

        if let Some(ref name) = self.selected_server {
            let server = self.servers.iter().find(|s| match s {
                StoredServer::Oidc(s) => s.name == *name,
                StoredServer::Password(s) => s.name == *name,
            });

            if let Some(server) = server {
                match server {
                    StoredServer::Password(_) => {
                        let _ = self.cmd_tx.send(Command::ConnectPassword {
                            server_name: name.clone(),
                        });
                    }
                    StoredServer::Oidc(_) => {
                        let _ = self.cmd_tx.send(Command::ConnectOidc {
                            server_name: name.clone(),
                        });
                    }
                }
            }
        }
    }
}

/// Helper to create a null-terminated UTF-16 Vec from a string.
fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_child(
    parent: HWND,
    class: &[u16],
    text: &[u16],
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    style: WINDOW_STYLE,
) -> HWND {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR::from_raw(class.as_ptr()),
            PCWSTR::from_raw(text.as_ptr()),
            style | WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            parent,
            Some(WindowsAndMessaging::HMENU(id as usize)),
            None,
            None,
        )
    }
}

fn create_controls(hwnd: HWND) {
    let font = unsafe { Controls::GetStockObject(Controls::DEFAULT_GUI_FONT) };

    // Status bar
    let status = create_child(
        hwnd,
        Controls::STATIC,
        w!("Disconnected"),
        IDC_STATUS_LABEL,
        10, 5, 500, 25,
        WINDOW_STYLE(0),
    );
    unsafe { let _ = SendMessageW(status, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // === Disconnected panel ===
    let p1 = create_child(
        hwnd,
        Controls::STATIC,
        w!(""),
        IDC_PANEL_DISCONNECTED,
        10, 40, 680, 450,
        WINDOW_STYLE(0),
    );

    create_child(p1, Controls::STATIC, w!("Server:"), 0, 15, 12, 55, 25, WINDOW_STYLE(0));

    let combo = create_child(
        p1,
        Controls::COMBOBOX,
        w!(""),
        IDC_SERVER_COMBO,
        75, 10, 350, 200,
        Controls::CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP,
    );
    unsafe { let _ = SendMessageW(combo, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    let manage = create_child(
        p1,
        Controls::BUTTON,
        w!("Manage Servers"),
        IDC_MANAGE_BTN,
        435, 9, 120, 28,
        WS_TABSTOP,
    );
    unsafe { let _ = SendMessageW(manage, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    let st = create_child(p1, Controls::STATIC, w!(""), IDC_SERVER_TYPE_LABEL, 15, 55, 400, 22, WINDOW_STYLE(0));
    unsafe { let _ = SendMessageW(st, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    let su = create_child(p1, Controls::STATIC, w!(""), IDC_SERVER_URL_LABEL, 15, 82, 650, 22, WINDOW_STYLE(0));
    unsafe { let _ = SendMessageW(su, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    let se = create_child(p1, Controls::STATIC, w!(""), IDC_SERVER_EXTRA_LABEL, 15, 109, 650, 22, WINDOW_STYLE(0));
    unsafe { let _ = SendMessageW(se, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    let btn = create_child(p1, Controls::BUTTON, w!("Connect"), IDC_CONNECT_BTN, 250, 160, 200, 35, WS_TABSTOP);
    unsafe {
        let _ = SendMessageW(btn, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
        let _ = EnableWindow(btn, false);
    }

    // === Connecting panel ===
    let p2 = create_child(hwnd, Controls::STATIC, w!(""), IDC_PANEL_CONNECTING, 10, 40, 680, 450, WINDOW_STYLE(0));
    let ct = create_child(p2, Controls::STATIC, w!("Connecting..."), IDC_PANEL_CONNECTING_TEXT, 50, 80, 580, 60, WINDOW_STYLE(0));
    unsafe { let _ = SendMessageW(ct, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // === Connected panel ===
    let p3 = create_child(hwnd, Controls::STATIC, w!(""), IDC_PANEL_CONNECTED, 10, 40, 680, 450, WINDOW_STYLE(0));
    let cd = create_child(p3, Controls::STATIC, w!("Connected"), IDC_PANEL_CONNECTED_TEXT, 50, 80, 580, 60, WINDOW_STYLE(0));
    unsafe { let _ = SendMessageW(cd, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // === Error panel ===
    let p4 = create_child(hwnd, Controls::STATIC, w!(""), IDC_PANEL_ERROR, 10, 40, 680, 450, WINDOW_STYLE(0));
    let et = create_child(p4, Controls::STATIC, w!(""), IDC_PANEL_ERROR_TEXT, 50, 80, 580, 100, WINDOW_STYLE(0));
    unsafe { let _ = SendMessageW(et, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // Hide non-default panels
    unsafe {
        ShowWindow(p2, SW_HIDE);
        ShowWindow(p3, SW_HIDE);
        ShowWindow(p4, SW_HIDE);
    }
}

pub unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam.0 as *const WindowsAndMessaging::CREATESTRUCTW;
            let state_ptr = (*cs).lpCreateParams as *mut WindowState;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            create_controls(hwnd);

            let _ = (*state_ptr).cmd_tx.send(Command::GetConfigs);

            LRESULT(0)
        }
        WM_COMMAND => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;
            let ctrl_id = (wparam.0 & 0xFFFF) as i32;
            let notify = ((wparam.0 >> 16) & 0xFFFF) as u32;

            match ctrl_id {
                IDC_SERVER_COMBO if notify == CBN_SELCHANGE.0 => {
                    state.handle_combo_change();
                }
                IDC_CONNECT_BTN => {
                    state.handle_connect();
                }
                IDC_MANAGE_BTN => {
                    crate::dialog::open_server_editor(
                        hwnd,
                        &state.cmd_tx,
                        &state.servers,
                        state.selected_server.clone(),
                    );
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_REFRESH_UI => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;

            if let Ok(rx) = state.event_rx.lock() {
                loop {
                    match rx.try_recv() {
                        Ok(Event::StatusChanged { status, message }) => {
                            state.current_status = status.clone();
                            state.set_status_text(&status, message.as_deref());
                            match status.as_str() {
                                "CONNECTING" => {
                                    let txt: HSTRING =
                                        format!("Connecting...\n{}", message.as_deref().unwrap_or(""))
                                            .into();
                                    unsafe {
                                        let _ = SetWindowTextW(
                                            GetDlgItem(hwnd, IDC_PANEL_CONNECTING_TEXT),
                                            &txt,
                                        );
                                    }
                                }
                                "DISCONNECTING" => {
                                    state.set_connecting_text("Disconnecting...");
                                }
                                "CONNECTED" => {
                                    state.set_connected_text("Connected to VPN");
                                }
                                "ERROR" => {
                                    state.set_error_text(
                                        message.as_deref().unwrap_or("Unknown error"),
                                    );
                                }
                                _ => {}
                            }
                            state.refresh_ui();
                        }
                        Ok(Event::ConfigsLoaded { servers, default }) => {
                            state.servers = servers;
                            state.selected_server = default.or_else(|| {
                                state.servers.first().map(|s| match s {
                                    StoredServer::Oidc(s) => s.name.clone(),
                                    StoredServer::Password(s) => s.name.clone(),
                                })
                            });
                            state.update_server_combo();
                            state.update_server_info();
                            state.refresh_ui();
                        }
                        Ok(Event::Error(err)) => {
                            state.set_status_text("ERROR", Some(&err));
                            state.set_error_text(&err);
                            state.current_status = "ERROR".to_string();
                            state.refresh_ui();
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !state_ptr.is_null() {
                let _ = Box::from_raw(state_ptr);
            }
            let _ = WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn create_main_window(
    cmd_tx: Sender<Command>,
    event_rx: Receiver<Event>,
    icon: WindowsAndMessaging::HICON,
) -> HWND {
    let hinstance = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap_or(windows::Win32::Foundation::HMODULE(std::ptr::null_mut()))
    };

    let wc = WNDCLASSW {
        style: WindowsAndMessaging::CS_HREDRAW | WindowsAndMessaging::CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap() },
        hbrBackground: unsafe { GetStockObject(Controls::WHITE_BRUSH) },
        lpszClassName: w!("OpenConnectVPNWin"),
        hIcon: icon,
        ..Default::default()
    };

    unsafe {
        let _ = WindowsAndMessaging::RegisterClassW(&wc);
    }

    let state = Box::new(WindowState {
        hwnd: HWND(std::ptr::null_mut()),
        cmd_tx: cmd_tx.clone(),
        event_rx: Mutex::new(event_rx),
        servers: Vec::new(),
        selected_server: None,
        current_status: "INITIALIZED".to_string(),
    });

    let state_ptr = Box::into_raw(state);

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("OpenConnectVPNWin"),
            w!("OpenConnect VPN"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            WindowsAndMessaging::CW_USEDEFAULT,
            WindowsAndMessaging::CW_USEDEFAULT,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            None,
            None,
            hinstance,
            Some(state_ptr as *mut std::ffi::c_void),
        )
    };

    unsafe {
        (*state_ptr).hwnd = hwnd;
    }

    hwnd
}
