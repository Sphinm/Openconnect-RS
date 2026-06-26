use crate::message::Command;
use openconnect_core::storage::{OidcServer, PasswordServer, StoredServer};
use std::sync::mpsc::Sender;
use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Controls::{self, CBN_SELCHANGE},
            WindowsAndMessaging::{
                self, CreateWindowExW, DefWindowProcW, DestroyWindow, EnableWindow,
                GetDlgItem, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
                LoadCursorW, MessageBoxW, SendMessageW, SetWindowLongPtrW, SetWindowTextW,
                GWLP_USERDATA, IDC_ARROW, MB_ICONQUESTION, MB_OKCANCEL, MB_YESNO,
                SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
                WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
                WS_BORDER, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN,
                WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
};

// Dialog control IDs
const IDC_DLG_NAME: i32 = 2001;
const IDC_DLG_SERVER_URL: i32 = 2002;
const IDC_DLG_AUTH_TYPE: i32 = 2003;
const IDC_DLG_USERNAME: i32 = 2004;
const IDC_DLG_PASSWORD: i32 = 2005;
const IDC_DLG_ISSUER: i32 = 2006;
const IDC_DLG_CLIENT_ID: i32 = 2007;
const IDC_DLG_CLIENT_SECRET: i32 = 2008;
const IDC_DLG_ALLOW_INSECURE: i32 = 2009;
const IDC_DLG_SET_DEFAULT: i32 = 2010;
const IDC_DLG_SAVE: i32 = 2011;
const IDC_DLG_DELETE: i32 = 2012;
const IDC_DLG_CANCEL: i32 = 2013;
const IDC_DLG_IMPORT: i32 = 2014;
const IDC_DLG_PASSWORD_GROUP: i32 = 2015;
const IDC_DLG_OIDC_GROUP: i32 = 2016;

#[derive(Clone, PartialEq)]
enum AuthType {
    Password,
    Oidc,
}

struct DialogState {
    parent: HWND,
    hwnd: HWND,
    cmd_tx: Sender<Command>,
    mode: DialogMode,
    auth_type: AuthType,
}

enum DialogMode {
    Add,
    Edit { name: String },
}

impl DialogState {
    fn get_text(&self, id: i32) -> String {
        unsafe {
            let hwnd = GetDlgItem(self.hwnd, id);
            let len = GetWindowTextLengthW(hwnd) as usize;
            if len == 0 {
                return String::new();
            }
            let mut buf: Vec<u16> = vec![0; len + 1];
            let _ = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..len])
        }
    }

    fn set_text(&self, id: i32, text: &str) {
        unsafe {
            let hwnd = GetDlgItem(self.hwnd, id);
            let s: HSTRING = text.into();
            let _ = SetWindowTextW(hwnd, &s);
        }
    }

    fn is_checked(&self, id: i32) -> bool {
        unsafe {
            let hwnd = GetDlgItem(self.hwnd, id);
            SendMessageW(hwnd, Controls::BM_GETCHECK, None, None).0 == Controls::BST_CHECKED.0 as isize
        }
    }

    fn set_checked(&self, id: i32, checked: bool) {
        unsafe {
            let hwnd = GetDlgItem(self.hwnd, id);
            let _ = SendMessageW(
                hwnd,
                Controls::BM_SETCHECK,
                Some(WPARAM(if checked { Controls::BST_CHECKED.0 as usize } else { Controls::BST_UNCHECKED.0 as usize })),
                None,
            );
        }
    }

    fn switch_auth_type(&mut self, auth_type: AuthType) {
        self.auth_type = auth_type;
        unsafe {
            let combo = GetDlgItem(self.hwnd, IDC_DLG_AUTH_TYPE);
            let idx = match auth_type {
                AuthType::Password => 0,
                AuthType::Oidc => 1,
            };
            let _ = SendMessageW(combo, Controls::CB_SETCURSEL, Some(WPARAM(idx)), None);

            let show_password = if auth_type == AuthType::Password { SW_SHOW } else { SW_HIDE };
            let show_oidc = if auth_type == AuthType::Oidc { SW_SHOW } else { SW_HIDE };

            ShowWindow(GetDlgItem(self.hwnd, IDC_DLG_PASSWORD_GROUP), show_password);
            ShowWindow(GetDlgItem(self.hwnd, IDC_DLG_OIDC_GROUP), show_oidc);
        }
    }

    fn on_save(&self) {
        let name = self.get_text(IDC_DLG_NAME);
        let server_url = self.get_text(IDC_DLG_SERVER_URL);
        let allow_insecure = self.is_checked(IDC_DLG_ALLOW_INSECURE);

        if name.is_empty() || server_url.is_empty() {
            unsafe {
                let _ = MessageBoxW(
                    self.hwnd,
                    w!("Name and Server URL are required."),
                    w!("Validation Error"),
                    MB_OKCANCEL,
                );
            }
            return;
        }

        let stored_server = match self.auth_type {
            AuthType::Password => {
                let username = self.get_text(IDC_DLG_USERNAME);
                let password = self.get_text(IDC_DLG_PASSWORD);
                StoredServer::Password(PasswordServer {
                    name,
                    server: server_url,
                    username,
                    password: if password.is_empty() { None } else { Some(password) },
                    allow_insecure: Some(allow_insecure),
                    updated_at: None,
                })
            }
            AuthType::Oidc => {
                let issuer = self.get_text(IDC_DLG_ISSUER);
                let client_id = self.get_text(IDC_DLG_CLIENT_ID);
                let client_secret = self.get_text(IDC_DLG_CLIENT_SECRET);

                if issuer.is_empty() || client_id.is_empty() {
                    unsafe {
                        let _ = MessageBoxW(
                            self.hwnd,
                            w!("Issuer and Client ID are required for OIDC."),
                            w!("Validation Error"),
                            MB_OKCANCEL,
                        );
                    }
                    return;
                }

                StoredServer::Oidc(OidcServer {
                    name,
                    server: server_url,
                    issuer,
                    client_id,
                    client_secret: if client_secret.is_empty() { None } else { Some(client_secret) },
                    allow_insecure: Some(allow_insecure),
                    updated_at: None,
                })
            }
        };

        let _ = self.cmd_tx.send(Command::UpsertServer(stored_server));

        if self.is_checked(IDC_DLG_SET_DEFAULT) {
            let default_name = self.get_text(IDC_DLG_NAME);
            let _ = self.cmd_tx.send(Command::SetDefault(default_name));
        }

        unsafe {
            let _ = EnableWindow(self.parent, true);
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn on_delete(&self) {
        if let DialogMode::Edit { ref name } = self.mode {
            let msg: HSTRING = format!(
                "Are you sure you want to delete server '{}'?",
                name
            )
            .into();
            let result = unsafe {
                MessageBoxW(
                    self.hwnd,
                    &msg,
                    w!("Confirm Delete"),
                    MB_YESNO | MB_ICONQUESTION,
                )
            };
            if result == windows::Win32::UI::WindowsAndMessaging::IDYES {
                let _ = self.cmd_tx.send(Command::RemoveServer(name.clone()));
                unsafe {
                    let _ = EnableWindow(self.parent, true);
                    let _ = DestroyWindow(self.hwnd);
                }
            }
        }
    }

    fn on_cancel(&self) {
        unsafe {
            let _ = EnableWindow(self.parent, true);
            let _ = DestroyWindow(self.hwnd);
        }
    }

    fn on_import(&self) {
        // Try to read base64 from clipboard
        unsafe {
            if !WindowsAndMessaging::OpenClipboard(self.hwnd).as_bool() {
                return;
            }
            let handle = WindowsAndMessaging::GetClipboardData(1u32); // CF_TEXT
            if let Ok(handle) = handle {
                let ptr = windows::Win32::System::Memory::GlobalLock(
                    windows::Win32::Foundation::HGLOBAL(handle.0),
                );
                if let Ok(ptr) = ptr {
                    let cstr = std::ffi::CStr::from_ptr(ptr as *const i8);
                    if let Ok(b64) = cstr.to_str() {
                        let b64 = b64.trim();
                        if let Ok(bytes) = base64_decode(b64) {
                            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                // Try to parse as imported server config
                                if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                                    self.set_text(IDC_DLG_NAME, name);
                                }
                                if let Some(server) = json.get("server").and_then(|v| v.as_str()) {
                                    self.set_text(IDC_DLG_SERVER_URL, server);
                                }
                                if let Some(auth_type) = json.get("authType").and_then(|v| v.as_str()) {
                                    match auth_type {
                                        "password" => self.switch_auth_type(AuthType::Password),
                                        "oidc" => self.switch_auth_type(AuthType::Oidc),
                                        _ => {}
                                    }
                                }
                                if let Some(u) = json.get("username").and_then(|v| v.as_str()) {
                                    self.set_text(IDC_DLG_USERNAME, u);
                                }
                                if let Some(p) = json.get("password").and_then(|v| v.as_str()) {
                                    self.set_text(IDC_DLG_PASSWORD, p);
                                }
                                if let Some(i) = json.get("issuer").and_then(|v| v.as_str()) {
                                    self.set_text(IDC_DLG_ISSUER, i);
                                }
                                if let Some(c) = json.get("clientId").and_then(|v| v.as_str()) {
                                    self.set_text(IDC_DLG_CLIENT_ID, c);
                                }
                                if let Some(s) = json.get("clientSecret").and_then(|v| v.as_str()) {
                                    self.set_text(IDC_DLG_CLIENT_SECRET, s);
                                }
                                if let Some(a) = json.get("allowInsecure").and_then(|v| v.as_bool()) {
                                    self.set_checked(IDC_DLG_ALLOW_INSECURE, a);
                                }
                            }
                        }
                    }
                    let _ = windows::Win32::System::Memory::GlobalUnlock(
                        windows::Win32::Foundation::HGLOBAL(handle.0),
                    );
                }
            }
            let _ = WindowsAndMessaging::CloseClipboard();
        }
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    // Simple base64 decode without external crate
    // Use a minimal implementation
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.trim_end_matches('=');
    let mut output = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;

    for c in input.chars() {
        let val = CHARS.iter().position(|&x| x as char == c).ok_or(())? as u32;
        buffer = (buffer << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(output)
}

fn create_dialog_control(
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

fn create_dialog_controls(hwnd: HWND) {
    let font = unsafe { Controls::GetStockObject(Controls::DEFAULT_GUI_FONT) };

    // Name
    create_dialog_control(hwnd, Controls::STATIC, w!("Name:"), 0, 15, 15, 100, 22, WINDOW_STYLE(0));
    let name = create_dialog_control(hwnd, Controls::EDIT, w!(""), IDC_DLG_NAME, 120, 13, 280, 24, WS_TABSTOP | WS_BORDER);
    unsafe { let _ = SendMessageW(name, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // Server URL
    create_dialog_control(hwnd, Controls::STATIC, w!("Server URL:"), 0, 15, 48, 100, 22, WINDOW_STYLE(0));
    let url = create_dialog_control(hwnd, Controls::EDIT, w!(""), IDC_DLG_SERVER_URL, 120, 46, 280, 24, WS_TABSTOP | WS_BORDER);
    unsafe { let _ = SendMessageW(url, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // Auth Type
    create_dialog_control(hwnd, Controls::STATIC, w!("Auth Type:"), 0, 15, 81, 100, 22, WINDOW_STYLE(0));
    let auth_combo = create_dialog_control(
        hwnd,
        Controls::COMBOBOX,
        w!(""),
        IDC_DLG_AUTH_TYPE,
        120, 79, 150, 100,
        Controls::CBS_DROPDOWNLIST | WS_TABSTOP,
    );
    unsafe {
        let _ = SendMessageW(auth_combo, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None);
        let _ = SendMessageW(auth_combo, Controls::CB_ADDSTRING, None, Some(LPARAM(w!("Password\0").as_ptr() as isize)));
        let _ = SendMessageW(auth_combo, Controls::CB_ADDSTRING, None, Some(LPARAM(w!("OIDC\0").as_ptr() as isize)));
        let _ = SendMessageW(auth_combo, Controls::CB_SETCURSEL, Some(WPARAM(0)), None);
    }

    // --- Password group ---
    let pw_group = create_dialog_control(hwnd, Controls::BUTTON, w!("Password Auth"), IDC_DLG_PASSWORD_GROUP, 15, 115, 390, 80, Controls::BS_GROUPBOX);
    unsafe { let _ = SendMessageW(pw_group, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    create_dialog_control(pw_group, Controls::STATIC, w!("Username:"), 0, 30, 20, 80, 22, WINDOW_STYLE(0));
    let uname = create_dialog_control(pw_group, Controls::EDIT, w!(""), IDC_DLG_USERNAME, 105, 18, 270, 24, WS_TABSTOP | WS_BORDER);
    unsafe { let _ = SendMessageW(uname, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    create_dialog_control(pw_group, Controls::STATIC, w!("Password:"), 0, 30, 50, 80, 22, WINDOW_STYLE(0));
    let pwd = create_dialog_control(
        pw_group,
        Controls::EDIT,
        w!(""),
        IDC_DLG_PASSWORD,
        105, 48, 270, 24,
        Controls::ES_PASSWORD | WS_TABSTOP | WS_BORDER,
    );
    unsafe { let _ = SendMessageW(pwd, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // --- OIDC group ---
    let oidc_group = create_dialog_control(hwnd, Controls::BUTTON, w!("OIDC Auth"), IDC_DLG_OIDC_GROUP, 15, 115, 390, 140, Controls::BS_GROUPBOX);
    unsafe { let _ = SendMessageW(oidc_group, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    create_dialog_control(oidc_group, Controls::STATIC, w!("Issuer URL:"), 0, 30, 20, 80, 22, WINDOW_STYLE(0));
    let issuer = create_dialog_control(oidc_group, Controls::EDIT, w!(""), IDC_DLG_ISSUER, 105, 18, 270, 24, WS_TABSTOP | WS_BORDER);
    unsafe { let _ = SendMessageW(issuer, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    create_dialog_control(oidc_group, Controls::STATIC, w!("Client ID:"), 0, 30, 50, 80, 22, WINDOW_STYLE(0));
    let cid = create_dialog_control(oidc_group, Controls::EDIT, w!(""), IDC_DLG_CLIENT_ID, 105, 48, 270, 24, WS_TABSTOP | WS_BORDER);
    unsafe { let _ = SendMessageW(cid, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    create_dialog_control(oidc_group, Controls::STATIC, w!("Client Secret:"), 0, 30, 80, 80, 22, WINDOW_STYLE(0));
    let cs = create_dialog_control(
        oidc_group,
        Controls::EDIT,
        w!(""),
        IDC_DLG_CLIENT_SECRET,
        105, 78, 270, 24,
        Controls::ES_PASSWORD | WS_TABSTOP | WS_BORDER,
    );
    unsafe { let _ = SendMessageW(cs, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // Hide OIDC group initially
    unsafe { ShowWindow(oidc_group, SW_HIDE); }

    // Allow insecure checkbox
    let cb = create_dialog_control(
        hwnd,
        Controls::BUTTON,
        w!("Allow insecure certificate"),
        IDC_DLG_ALLOW_INSECURE,
        15, 270, 220, 24,
        Controls::BS_AUTOCHECKBOX | WS_TABSTOP,
    );
    unsafe { let _ = SendMessageW(cb, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // Set as default checkbox
    let sb = create_dialog_control(
        hwnd,
        Controls::BUTTON,
        w!("Set as default server"),
        IDC_DLG_SET_DEFAULT,
        250, 270, 160, 24,
        Controls::BS_AUTOCHECKBOX | WS_TABSTOP,
    );
    unsafe { let _ = SendMessageW(sb, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // Import button
    let import = create_dialog_control(hwnd, Controls::BUTTON, w!("Import from Clipboard"), IDC_DLG_IMPORT, 15, 305, 160, 30, WS_TABSTOP);
    unsafe { let _ = SendMessageW(import, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    // Bottom buttons
    let save = create_dialog_control(hwnd, Controls::BUTTON, w!("Save"), IDC_DLG_SAVE, 120, 305, 90, 30, WS_TABSTOP);
    unsafe { let _ = SendMessageW(save, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    let delete = create_dialog_control(hwnd, Controls::BUTTON, w!("Delete"), IDC_DLG_DELETE, 220, 305, 90, 30, WS_TABSTOP);
    unsafe { let _ = SendMessageW(delete, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }

    let cancel = create_dialog_control(hwnd, Controls::BUTTON, w!("Cancel"), IDC_DLG_CANCEL, 320, 305, 90, 30, WS_TABSTOP);
    unsafe { let _ = SendMessageW(cancel, Controls::WM_SETFONT, Some(WPARAM(font.0 as usize)), None); }
}

unsafe extern "system" fn dialog_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam.0 as *const WindowsAndMessaging::CREATESTRUCTW;
            let state_ptr = (*cs).lpCreateParams as *mut DialogState;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
            (*state_ptr).hwnd = hwnd;

            create_dialog_controls(hwnd);

            // If editing, populate fields
            if let DialogMode::Edit { ref name } = (*state_ptr).mode {
                // The servers list was passed via some mechanism - we rely on the
                // callback populating. The caller should populate after creation.
            }

            // Disable delete button in Add mode
            if matches!((*state_ptr).mode, DialogMode::Add) {
                let _ = EnableWindow(GetDlgItem(hwnd, IDC_DLG_DELETE), false);
            }

            LRESULT(0)
        }
        WM_COMMAND => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let state = &mut *state_ptr;
            let ctrl_id = (wparam.0 & 0xFFFF) as i32;
            let notify = ((wparam.0 >> 16) & 0xFFFF) as u32;

            match ctrl_id {
                IDC_DLG_AUTH_TYPE if notify == CBN_SELCHANGE.0 => {
                    let combo = GetDlgItem(hwnd, ctrl_id);
                    let idx = SendMessageW(combo, Controls::CB_GETCURSEL, None, None).0;
                    let auth_type = if idx == 1 { AuthType::Oidc } else { AuthType::Password };
                    state.switch_auth_type(auth_type);
                }
                IDC_DLG_SAVE => state.on_save(),
                IDC_DLG_DELETE => state.on_delete(),
                IDC_DLG_CANCEL => state.on_cancel(),
                IDC_DLG_IMPORT => state.on_import(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
            if !state_ptr.is_null() {
                (*state_ptr).on_cancel();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
            if !state_ptr.is_null() {
                let _ = EnableWindow((*state_ptr).parent, true);
                let _ = Box::from_raw(state_ptr);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn open_server_editor(
    parent: HWND,
    cmd_tx: &Sender<Command>,
    servers: &[StoredServer],
    selected_name: Option<String>,
) {
    let _ = EnableWindow(parent, false);

    let hinstance = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .unwrap_or(windows::Win32::Foundation::HMODULE(std::ptr::null_mut()))
    };

    // Register dialog window class
    let wc = WNDCLASSW {
        style: WindowsAndMessaging::CS_HREDRAW | WindowsAndMessaging::CS_VREDRAW,
        lpfnWndProc: Some(dialog_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap() },
        hbrBackground: unsafe { GetStockObject(Controls::WHITE_BRUSH) },
        lpszClassName: w!("OpenConnectVPNDialog"),
        ..Default::default()
    };
    unsafe { let _ = WindowsAndMessaging::RegisterClassW(&wc); }

    let mode = if let Some(ref name) = selected_name {
        DialogMode::Edit { name: name.clone() }
    } else {
        DialogMode::Add
    };

    let auth_type = AuthType::Password; // default

    let mut state = Box::new(DialogState {
        parent,
        hwnd: HWND(std::ptr::null_mut()),
        cmd_tx: cmd_tx.clone(),
        mode,
        auth_type: auth_type.clone(),
    });

    // Find selected server data to populate fields (Edit mode)
    if let DialogMode::Edit { ref name } = state.mode {
        if let Some(server) = servers.iter().find(|s| match s {
            StoredServer::Oidc(s) => s.name == *name,
            StoredServer::Password(s) => s.name == *name,
        }) {
            // We'll set the state.auth_type now
            match server {
                StoredServer::Password(s) => {
                    state.auth_type = AuthType::Password;
                }
                StoredServer::Oidc(s) => {
                    state.auth_type = AuthType::Oidc;
                }
            }
        }
    }

    let state_ptr = Box::into_raw(state);

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("OpenConnectVPNDialog"),
            w!("Server Configuration"),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
            WindowsAndMessaging::CW_USEDEFAULT,
            WindowsAndMessaging::CW_USEDEFAULT,
            430,
            400,
            parent,
            None,
            hinstance,
            Some(state_ptr as *mut std::ffi::c_void),
        )
    };

    // Populate fields in edit mode
    if let DialogMode::Edit { ref name } = unsafe { (*state_ptr).mode.clone() } {
        if let Some(server) = servers.iter().find(|s| match s {
            StoredServer::Oidc(s) => s.name == *name,
            StoredServer::Password(s) => s.name == *name,
        }) {
            unsafe {
                let state = &mut *state_ptr;
                match server {
                    StoredServer::Password(s) => {
                        state.set_text(IDC_DLG_NAME, &s.name);
                        state.set_text(IDC_DLG_SERVER_URL, &s.server);
                        state.set_text(IDC_DLG_USERNAME, &s.username);
                        if let Some(ref pwd) = s.password {
                            state.set_text(IDC_DLG_PASSWORD, pwd);
                        }
                        state.set_checked(IDC_DLG_ALLOW_INSECURE, s.allow_insecure.unwrap_or(false));
                        state.switch_auth_type(AuthType::Password);
                    }
                    StoredServer::Oidc(s) => {
                        state.set_text(IDC_DLG_NAME, &s.name);
                        state.set_text(IDC_DLG_SERVER_URL, &s.server);
                        state.set_text(IDC_DLG_ISSUER, &s.issuer);
                        state.set_text(IDC_DLG_CLIENT_ID, &s.client_id);
                        if let Some(ref cs) = s.client_secret {
                            state.set_text(IDC_DLG_CLIENT_SECRET, cs);
                        }
                        state.set_checked(IDC_DLG_ALLOW_INSECURE, s.allow_insecure.unwrap_or(false));
                        state.switch_auth_type(AuthType::Oidc);
                    }
                }
            }
        }
    }

    unsafe {
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        ShowWindow(hwnd, SW_SHOW);
    }

    // Modal message loop
    unsafe {
        let mut msg = WindowsAndMessaging::MSG::default();
        loop {
            let ret = WindowsAndMessaging::GetMessageW(&mut msg, None, 0, 0);
            if ret.0 == 0 || ret.0 == -1 {
                break;
            }
            let _ = WindowsAndMessaging::TranslateMessage(&msg);
            WindowsAndMessaging::DispatchMessageW(&msg);
        }
        // Re-enable parent
        let _ = EnableWindow(parent, true);
    }
}
