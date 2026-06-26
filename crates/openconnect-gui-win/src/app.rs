use crate::message::{Command, Event};
use openconnect_core::{
    config::{ConfigBuilder, EntrypointBuilder, LogLevel},
    events::EventHandlers,
    storage::{StoredConfigs, StoredServer},
    Connectable, Status, VpnClient,
};
use openconnect_oidc::{
    obtain_cookie_by_oidc_token,
    oidc_token::{OpenIDTokenAuth, OpenIDTokenAuthConfig, OIDC_REDIRECT_URI},
};
use std::{
    path::PathBuf,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

// Must match the value defined in window.rs
const WM_REFRESH_UI: u32 = 0x400 + 1; // WM_USER + 1

/// Shared state so the background thread can post UI refresh messages.
pub struct SharedState {
    pub hwnd: Mutex<Option<HWND>>,
}

/// Background state held on the tokio runtime thread.
pub struct App {
    cmd_rx: Receiver<Command>,
    event_tx: Sender<Event>,
    client: Option<Arc<VpnClient>>,
    configs: StoredConfigs,
    shared: Arc<SharedState>,
}

impl App {
    pub fn new(
        cmd_rx: Receiver<Command>,
        event_tx: Sender<Event>,
        config_file: PathBuf,
        shared: Arc<SharedState>,
    ) -> anyhow::Result<Self> {
        openconnect_core::log::Logger::init()?;

        Ok(Self {
            cmd_rx,
            event_tx,
            client: None,
            configs: StoredConfigs::new(None, config_file),
            shared,
        })
    }

    /// Post a refresh message to the UI thread so it checks for pending events.
    fn post_refresh(&self) {
        if let Ok(guard) = self.shared.hwnd.lock() {
            if let Some(hwnd) = *guard {
                unsafe {
                    let _ = PostMessageW(hwnd, WM_REFRESH_UI, WPARAM(0), LPARAM(0));
                }
            }
        }
    }

    /// Send an event and wake up the UI thread.
    fn send_event(&self, event: Event) {
        let _ = self.event_tx.send(event);
        self.post_refresh();
    }

    /// Run the main command-processing loop on the current (tokio) runtime.
    pub async fn run(&mut self) {
        self.configs
            .read_from_file()
            .await
            .expect("Failed to read config file");

        loop {
            let cmd = match self.cmd_rx.recv() {
                Ok(cmd) => cmd,
                Err(_) => break, // channel closed
            };
            self.handle_command(cmd).await;
        }
    }

    async fn handle_command(&mut self, cmd: Command) {
        match cmd {
            Command::GetConfigs => {
                let servers: Vec<StoredServer> =
                    self.configs.servers.values().cloned().collect();
                self.send_event(Event::ConfigsLoaded {
                    servers,
                    default: self.configs.default.clone(),
                });
            }
            Command::ConnectPassword { server_name } => {
                if let Err(e) = self.connect_with_password(&server_name).await {
                    self.send_event(Event::Error(e.to_string()));
                }
            }
            Command::ConnectOidc { server_name } => {
                if let Err(e) = self.connect_with_oidc(&server_name).await {
                    self.send_event(Event::Error(e.to_string()));
                }
            }
            Command::Disconnect => {
                if let Err(e) = self.disconnect().await {
                    self.send_event(Event::Error(e.to_string()));
                }
            }
            Command::UpsertServer(server) => {
                if let Err(e) = self.configs.upsert_server(server).await {
                    self.send_event(Event::Error(e.to_string()));
                }
                let servers: Vec<StoredServer> =
                    self.configs.servers.values().cloned().collect();
                self.send_event(Event::ConfigsLoaded {
                    servers,
                    default: self.configs.default.clone(),
                });
            }
            Command::SetDefault(name) => {
                if let Err(e) = self.configs.set_default_server(&name).await {
                    self.send_event(Event::Error(e.to_string()));
                }
            }
            Command::RemoveServer(name) => {
                if let Err(e) = self.configs.remove_server(&name).await {
                    self.send_event(Event::Error(e.to_string()));
                }
                let servers: Vec<StoredServer> =
                    self.configs.servers.values().cloned().collect();
                self.send_event(Event::ConfigsLoaded {
                    servers,
                    default: self.configs.default.clone(),
                });
            }
        }
    }

    async fn connect_with_password(&mut self, server_name: &str) -> Result<(), anyhow::Error> {
        let password_server = self
            .configs
            .get_server_as_password_server(server_name)?
            .decrypted_by(&self.configs.cipher);

        let config = ConfigBuilder::default().loglevel(LogLevel::Info).build()?;

        let entrypoint = EntrypointBuilder::new()
            .name(&password_server.name)
            .server(&password_server.server)
            .username(&password_server.username)
            .password(&password_server.password.unwrap_or_default())
            .accept_insecure_cert(password_server.allow_insecure.unwrap_or(false))
            .enable_udp(true)
            .build()?;

        let event_handlers = self.create_event_handler();

        let client = VpnClient::new(config, event_handlers)?;
        self.client = Some(client.clone());
        client.init_connection(entrypoint)?;

        let client_clone = client.clone();
        tokio::task::spawn_blocking(move || {
            let _ = client_clone.run_loop();
        });

        Ok(())
    }

    async fn connect_with_oidc(&mut self, server_name: &str) -> Result<(), anyhow::Error> {
        let oidc_server = self.configs.get_server_as_oidc_server(server_name)?;

        let openid_config = OpenIDTokenAuthConfig {
            issuer_url: oidc_server.issuer.clone(),
            redirect_uri: OIDC_REDIRECT_URI.to_string(),
            client_id: oidc_server.client_id.clone(),
            client_secret: oidc_server.client_secret.clone(),
            use_pkce_challenge: true,
        };

        let mut openid = OpenIDTokenAuth::new(openid_config).await?;
        let (authorize_url, req_state, _) = openid.auth_request();

        let _ = self.event_tx.send(Event::StatusChanged {
            status: "CONNECTING".to_string(),
            message: Some("Opening browser for authentication...".to_string()),
        });
        self.post_refresh();

        open::that(authorize_url.to_string())?;
        let (code, callback_state) = openid.wait_for_callback().await?;

        if req_state.secret() != callback_state.secret() {
            anyhow::bail!("OIDC state validation failed");
        }

        let token = openid.exchange_token(code).await?;
        let cookie = obtain_cookie_by_oidc_token(&oidc_server.server, &token)
            .await
            .ok_or_else(|| anyhow::anyhow!("Failed to obtain cookie"))?;

        let config = ConfigBuilder::default().loglevel(LogLevel::Info).build()?;

        let entrypoint = EntrypointBuilder::new()
            .name(&oidc_server.name)
            .server(&oidc_server.server)
            .cookie(&cookie)
            .accept_insecure_cert(oidc_server.allow_insecure.unwrap_or(false))
            .build()?;

        let event_handlers = self.create_event_handler();

        let client = VpnClient::new(config, event_handlers)?;
        self.client = Some(client.clone());
        client.init_connection(entrypoint)?;

        let client_clone = client.clone();
        tokio::task::spawn_blocking(move || {
            let _ = client_clone.run_loop();
        });

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), anyhow::Error> {
        if let Some(ref client) = self.client {
            let client = client.clone();
            tokio::task::spawn_blocking(move || client.disconnect()).await?;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        self.client = None;
        Ok(())
    }

    fn create_event_handler(&self) -> EventHandlers {
        let event_tx_for_state = self.event_tx.clone();
        let event_tx_for_cert = self.event_tx.clone();
        let shared_for_state = self.shared.clone();
        let shared_for_cert = self.shared.clone();

        let post_refresh = move || {
            if let Ok(guard) = shared_for_state.hwnd.lock() {
                if let Some(hwnd) = *guard {
                    unsafe {
                        let _ = PostMessageW(hwnd, WM_REFRESH_UI, WPARAM(0), LPARAM(0));
                    }
                }
            }
        };
        let post_refresh_cert = move || {
            if let Ok(guard) = shared_for_cert.hwnd.lock() {
                if let Some(hwnd) = *guard {
                    unsafe {
                        let _ = PostMessageW(hwnd, WM_REFRESH_UI, WPARAM(0), LPARAM(0));
                    }
                }
            }
        };

        EventHandlers::default()
            .with_handle_connection_state_change(move |state| {
                let (status, message) = match state {
                    Status::Initialized => ("INITIALIZED".to_string(), None),
                    Status::Connecting(msg) => ("CONNECTING".to_string(), Some(msg)),
                    Status::Connected => ("CONNECTED".to_string(), None),
                    Status::Disconnecting => ("DISCONNECTING".to_string(), None),
                    Status::Disconnected => ("DISCONNECTED".to_string(), None),
                    Status::Error(err) => ("ERROR".to_string(), Some(err.to_string())),
                };
                let _ = event_tx_for_state.send(Event::StatusChanged { status, message });
                post_refresh();
            })
            .with_handle_peer_cert_invalid(move |fingerprint| {
                let _ = event_tx_for_cert.send(Event::StatusChanged {
                    status: "ERROR".to_string(),
                    message: Some(format!(
                        "Peer certificate invalid. Enable 'Allow insecure' in server config to connect.\nFingerprint: {}",
                        fingerprint
                    )),
                });
                post_refresh_cert();
                false
            })
    }
}
