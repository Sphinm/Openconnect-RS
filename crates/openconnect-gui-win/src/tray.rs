use crate::message::Command;
use openconnect_core::storage::StoredServer;
use std::sync::mpsc::Sender;
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{
            Shell::{
                self, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
                NOTIFYICONDATAW,
            },
            WindowsAndMessaging::{
                self, AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos,
                PostMessageW, SetForegroundWindow, ShowWindow,
                TrackPopupMenu, HICON, MF_POPUP, MF_SEPARATOR, MF_STRING,
                SW_SHOW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD,
                WM_LBUTTONDBLCLK, WM_RBUTTONUP,
            },
        },
    },
};

pub const WM_TRAY_NOTIFY: u32 = WindowsAndMessaging::WM_USER + 100;

// Tray menu item IDs
const IDM_SHOW: usize = 3001;
const IDM_QUIT: usize = 3002;
const IDM_SERVER_BASE: usize = 3100; // 3100 + server_index

pub struct SystemTray {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    icon_connected: HICON,
    icon_disconnected: HICON,
}

impl SystemTray {
    pub fn new(
        hwnd: HWND,
        icon_connected: HICON,
        icon_disconnected: HICON,
    ) -> Self {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_NOTIFY;
        nid.hIcon = icon_disconnected;

        // Set tooltip
        let tip: Vec<u16> = "OpenConnect VPN"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let copy_len = nid.szTip.len().min(tip.len());
        nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);

        unsafe {
            let _ = Shell::Shell_NotifyIconW(NIM_ADD, &nid);
        }

        Self {
            hwnd,
            nid,
            icon_connected,
            icon_disconnected,
        }
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.nid.hIcon = if connected {
            self.icon_connected
        } else {
            self.icon_disconnected
        };
        self.nid.uFlags = NIF_ICON;
        unsafe {
            let _ = Shell::Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    pub fn show_menu(
        &self,
        servers: &[StoredServer],
        connected_server: Option<&str>,
        _connected: bool,
        cmd_tx: &Sender<Command>,
    ) {
        unsafe {
            let menu = CreatePopupMenu().unwrap();

            // Servers submenu
            let servers_menu = CreatePopupMenu().unwrap();

            for (i, server) in servers.iter().enumerate() {
                let (name, is_current) = match server {
                    StoredServer::Oidc(s) => (
                        s.name.clone(),
                        connected_server.map_or(false, |cs| cs == s.name),
                    ),
                    StoredServer::Password(s) => (
                        s.name.clone(),
                        connected_server.map_or(false, |cs| cs == s.name),
                    ),
                };

                let label = if is_current {
                    format!("Disconnect {}", name)
                } else {
                    format!("Connect {}", name)
                };

                let label_wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = AppendMenuW(
                    servers_menu,
                    MF_STRING,
                    IDM_SERVER_BASE + i,
                    windows::core::PCWSTR::from_raw(label_wide.as_ptr()),
                );
            }

            let servers_label: Vec<u16> = "Servers\0".encode_utf16().collect();
            let _ = AppendMenuW(
                menu,
                MF_STRING | MF_POPUP,
                servers_menu.0 as usize,
                windows::core::PCWSTR::from_raw(servers_label.as_ptr()),
            );

            // Separator
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);

            // Show Window
            let _ = AppendMenuW(menu, MF_STRING, IDM_SHOW, w!("Show Window"));

            // Quit
            let _ = AppendMenuW(menu, MF_STRING, IDM_QUIT, w!("Quit"));

            // Position at cursor
            let mut point = windows::Win32::Foundation::POINT::default();
            let _ = GetCursorPos(&mut point);

            let _ = SetForegroundWindow(self.hwnd);

            let selected = TrackPopupMenu(
                menu,
                TPM_BOTTOMALIGN | TPM_LEFTALIGN | TPM_RETURNCMD,
                point.x,
                point.y,
                0,
                self.hwnd,
                None,
            );

            let _ = DestroyMenu(menu);

            // Handle selection
            match selected.0 as usize {
                IDM_SHOW => {
                    let _ =
                        ShowWindow(self.hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(self.hwnd);
                }
                IDM_QUIT => {
                    // Disconnect first, then quit
                    let _ = cmd_tx.send(Command::Disconnect);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let _ = PostMessageW(self.hwnd, WindowsAndMessaging::WM_QUIT, WPARAM(0), LPARAM(0));
                }
                id if id >= IDM_SERVER_BASE => {
                    let idx = id - IDM_SERVER_BASE;
                    if let Some(server) = servers.get(idx) {
                        let name = match server {
                            StoredServer::Oidc(s) => s.name.clone(),
                            StoredServer::Password(s) => s.name.clone(),
                        };
                        let is_current = connected_server.map_or(false, |cs| cs == name);
                        if is_current {
                            let _ = cmd_tx.send(Command::Disconnect);
                        } else {
                            match server {
                                StoredServer::Password(_) => {
                                    let _ = cmd_tx.send(Command::ConnectPassword {
                                        server_name: name,
                                    });
                                }
                                StoredServer::Oidc(_) => {
                                    let _ = cmd_tx.send(Command::ConnectOidc {
                                        server_name: name,
                                    });
                                }
                            }
                        }
                    }
                }
                _ => {} // menu dismissed
            }
        }
    }
}

impl Drop for SystemTray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell::Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }
}

/// Handle tray notification message in the window proc.
/// Call this from the WM_TRAY_NOTIFY handler.
pub fn handle_tray_notify(
    hwnd: HWND,
    lparam: LPARAM,
    tray: &mut Option<SystemTray>,
    servers: &[StoredServer],
    connected_server: Option<&str>,
    _connected: bool,
    cmd_tx: &Sender<Command>,
) {
    let event = (lparam.0 & 0xFFFF) as u32;

    if event == WM_RBUTTONUP || event == WindowsAndMessaging::WM_CONTEXTMENU {
        if let Some(ref tray) = tray {
            tray.show_menu(servers, connected_server, _connected, cmd_tx);
        }
    } else if event == WM_LBUTTONDBLCLK {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}
