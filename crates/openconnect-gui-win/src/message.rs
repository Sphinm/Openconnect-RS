use openconnect_core::storage::StoredServer;

/// Commands sent from the UI thread to the background async task.
pub enum Command {
    ConnectPassword { server_name: String },
    ConnectOidc { server_name: String },
    Disconnect,
    GetConfigs,
    UpsertServer(StoredServer),
    SetDefault(String),
    RemoveServer(String),
}

/// Events sent from the background async task to the UI thread.
pub enum Event {
    StatusChanged {
        status: String,
        message: Option<String>,
    },
    ConfigsLoaded {
        servers: Vec<StoredServer>,
        default: Option<String>,
    },
    Error(String),
}
