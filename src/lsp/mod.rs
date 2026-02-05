// LSP 管理器：按用户/项目根目录管理 LSP 进程，提供诊断与代码导航能力。
use crate::config::{Config, LspServerConfig};
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio::time::{sleep, timeout, Duration, Instant};
use tracing::warn;
use url::Url;

const DEFAULT_TIMEOUT_S: u64 = 30;
const DEFAULT_DIAGNOSTICS_DEBOUNCE_MS: u64 = 150;
const DEFAULT_IDLE_TTL_S: u64 = 1800;
const CLEANUP_INTERVAL_S: u64 = 300;
const DIAGNOSTICS_WAIT_TIMEOUT_MS: u64 = 3000;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LspDiagnostic {
    #[serde(default)]
    pub range: LspRange,
    #[serde(default)]
    pub severity: Option<u32>,
    #[serde(default)]
    pub code: Option<Value>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub message: String,
}

impl LspDiagnostic {
    pub fn is_error(&self) -> bool {
        matches!(self.severity, Some(1))
    }

    pub fn pretty(&self) -> String {
        let severity = match self.severity.unwrap_or(1) {
            1 => "ERROR",
            2 => "WARN",
            3 => "INFO",
            4 => "HINT",
            _ => "INFO",
        };
        let line = self.range.start.line.saturating_add(1);
        let col = self.range.start.character.saturating_add(1);
        format!("{severity} [{line}:{col}] {}", self.message)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LspStatus {
    pub server_id: String,
    pub server_name: String,
    pub user_id: String,
    pub root: String,
    pub status: String,
    pub last_used_at: f64,
}

#[derive(Clone)]
pub struct LspManager {
    workspace: Arc<WorkspaceManager>,
    clients: DashMap<ClientKey, Arc<LspClient>>,
    spawn_lock: Arc<Mutex<()>>,
    idle_ttl_s: Arc<AtomicU64>,
}

impl LspManager {
    pub fn new(workspace: Arc<WorkspaceManager>) -> Arc<Self> {
        let manager = Arc::new(Self {
            workspace,
            clients: DashMap::new(),
            spawn_lock: Arc::new(Mutex::new(())),
            idle_ttl_s: Arc::new(AtomicU64::new(DEFAULT_IDLE_TTL_S)),
        });
        Self::start_cleanup_task(&manager);
        manager
    }

    fn start_cleanup_task(manager: &Arc<Self>) {
        let weak = Arc::downgrade(manager);
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(CLEANUP_INTERVAL_S)).await;
                let Some(manager) = weak.upgrade() else {
                    break;
                };
                manager.cleanup_idle().await;
            }
        });
    }

    pub async fn sync_with_config(&self, config: &Config) {
        let idle = config.lsp.idle_ttl_s;
        let idle = if idle == 0 { DEFAULT_IDLE_TTL_S } else { idle };
        self.idle_ttl_s.store(idle, Ordering::Relaxed);
        if !config.lsp.enabled {
            self.shutdown_all().await;
            return;
        }
        let enabled_servers: HashSet<String> = config
            .lsp
            .servers
            .iter()
            .filter(|server| server.enabled)
            .map(|server| server.id.clone())
            .collect();
        let keys = self
            .clients
            .iter()
            .filter_map(|entry| {
                if enabled_servers.contains(&entry.key().server_id) {
                    None
                } else {
                    Some(entry.key().clone())
                }
            })
            .collect::<Vec<_>>();
        for key in keys {
            self.remove_client(&key).await;
        }
    }

    pub fn status(&self) -> Vec<LspStatus> {
        let mut output = Vec::new();
        for entry in self.clients.iter() {
            let key = entry.key();
            let client = entry.value();
            let last_used = client.last_used_at();
            let root = self.workspace.display_path(&key.user_id, &client.root);
            output.push(LspStatus {
                server_id: key.server_id.clone(),
                server_name: client.server_name.clone(),
                user_id: key.user_id.clone(),
                root,
                status: if client.is_alive() {
                    "connected".to_string()
                } else {
                    "error".to_string()
                },
                last_used_at: last_used,
            });
        }
        output.sort_by(|a, b| a.server_id.cmp(&b.server_id));
        output
    }

    pub async fn touch_file(
        &self,
        config: &Config,
        user_id: &str,
        file_path: &Path,
        wait_for_diagnostics: bool,
    ) -> Result<()> {
        let clients = self.get_clients(config, user_id, file_path).await?;
        if clients.is_empty() {
            return Ok(());
        }
        for client in &clients {
            client.touch();
        }
        for client in clients {
            let target = file_path.to_path_buf();
            client.open_file(&target, wait_for_diagnostics).await?;
            if wait_for_diagnostics {
                let debounce = resolve_diagnostics_debounce_ms(config);
                client
                    .wait_for_diagnostics(&target, debounce, DIAGNOSTICS_WAIT_TIMEOUT_MS)
                    .await;
            }
        }
        Ok(())
    }

    pub(crate) async fn run_on_clients<F, Fut, T>(
        &self,
        config: &Config,
        user_id: &str,
        file_path: &Path,
        handler: F,
    ) -> Result<Vec<T>>
    where
        F: Fn(Arc<LspClient>) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let clients = self.get_clients(config, user_id, file_path).await?;
        if clients.is_empty() {
            return Err(anyhow!("no lsp client available"));
        }
        let mut results = Vec::new();
        for client in clients {
            client.touch();
            results.push(handler(client).await?);
        }
        Ok(results)
    }

    pub fn diagnostics_for_user(&self, user_id: &str) -> HashMap<PathBuf, Vec<LspDiagnostic>> {
        let mut output: HashMap<PathBuf, Vec<LspDiagnostic>> = HashMap::new();
        for entry in self.clients.iter() {
            if entry.key().user_id != user_id {
                continue;
            }
            for item in entry.value().diagnostics.iter() {
                output
                    .entry(item.key().clone())
                    .or_default()
                    .extend(item.value().clone());
            }
        }
        output
    }

    async fn cleanup_idle(&self) {
        let idle_ttl_s = self.idle_ttl_s.load(Ordering::Relaxed);
        if idle_ttl_s == 0 {
            return;
        }
        let now = now_ts();
        let keys = self
            .clients
            .iter()
            .filter_map(|entry| {
                let last_used = entry.value().last_used_at();
                let idle = if now >= last_used {
                    now - last_used
                } else {
                    0.0
                };
                if last_used > 0.0 && idle > idle_ttl_s as f64 {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for key in keys {
            self.remove_client(&key).await;
        }
    }

    async fn shutdown_all(&self) {
        let keys = self
            .clients
            .iter()
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        for key in keys {
            self.remove_client(&key).await;
        }
    }

    async fn remove_client(&self, key: &ClientKey) {
        if let Some((_, client)) = self.clients.remove(key) {
            client.shutdown().await;
        }
    }

    async fn get_clients(
        &self,
        config: &Config,
        user_id: &str,
        file_path: &Path,
    ) -> Result<Vec<Arc<LspClient>>> {
        if !config.lsp.enabled {
            return Ok(Vec::new());
        }
        let server_configs = resolve_servers_for_file(config, file_path);
        if server_configs.is_empty() {
            return Ok(Vec::new());
        }
        let user_root = self.workspace.workspace_root(user_id);
        if !is_within_root(&user_root, file_path) {
            return Err(anyhow!("file path out of workspace"));
        }
        let mut clients = Vec::new();
        for server in server_configs {
            let root = resolve_root_for_file(&user_root, file_path, &server.root_markers)
                .unwrap_or_else(|| user_root.clone());
            let key = ClientKey::new(user_id, &root, &server.id);
            let client = self
                .get_or_spawn_client(config, key, &server, &root)
                .await?;
            if client.is_alive() {
                clients.push(client);
            }
        }
        Ok(clients)
    }

    async fn get_or_spawn_client(
        &self,
        config: &Config,
        key: ClientKey,
        server: &LspServerConfig,
        root: &Path,
    ) -> Result<Arc<LspClient>> {
        if let Some(entry) = self.clients.get(&key) {
            if entry.value().is_alive() {
                return Ok(entry.value().clone());
            }
            self.clients.remove(&key);
        }
        let _guard = self.spawn_lock.lock().await;
        if let Some(entry) = self.clients.get(&key) {
            if entry.value().is_alive() {
                return Ok(entry.value().clone());
            }
            self.clients.remove(&key);
        }
        let client = LspClient::spawn(server, root, resolve_timeout_s(config)).await?;
        self.clients.insert(key, client.clone());
        Ok(client)
    }
}

#[derive(Clone, Debug)]
struct ClientKey {
    user_id: String,
    root: PathBuf,
    server_id: String,
}

impl ClientKey {
    fn new(user_id: &str, root: &Path, server_id: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            root: normalize_target_path(root),
            server_id: server_id.to_string(),
        }
    }
}

impl PartialEq for ClientKey {
    fn eq(&self, other: &Self) -> bool {
        self.user_id == other.user_id
            && normalize_path_for_compare(&self.root) == normalize_path_for_compare(&other.root)
            && self.server_id == other.server_id
    }
}

impl Eq for ClientKey {}

impl Hash for ClientKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.user_id.hash(state);
        normalize_path_for_compare(&self.root).hash(state);
        self.server_id.hash(state);
    }
}

struct OutgoingMessage {
    payload: Value,
}

pub(crate) struct LspClient {
    server_id: String,
    server_name: String,
    root: PathBuf,
    root_uri: String,
    initialization_options: Option<Value>,
    sender: mpsc::Sender<OutgoingMessage>,
    pending: DashMap<u64, oneshot::Sender<Result<Value>>>,
    diagnostics: DashMap<PathBuf, Vec<LspDiagnostic>>,
    diagnostics_tx: broadcast::Sender<PathBuf>,
    next_id: AtomicU64,
    last_used: AtomicU64,
    alive: AtomicBool,
    file_versions: Mutex<HashMap<PathBuf, i64>>,
    process: Mutex<Child>,
}

impl LspClient {
    async fn spawn(server: &LspServerConfig, root: &Path, timeout_s: u64) -> Result<Arc<Self>> {
        let command = normalize_command(&server.command)?;
        let mut cmd = Command::new(&command[0]);
        if command.len() > 1 {
            cmd.args(&command[1..]);
        }
        cmd.current_dir(root);
        for (key, value) in &server.env {
            cmd.env(key, value);
        }
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("LSP stdout missing"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("LSP stdin missing"))?;

        let (tx, mut rx) = mpsc::channel::<OutgoingMessage>(256);
        let (diag_tx, _) = broadcast::channel(64);
        let root_uri = path_to_uri(root)?;
        let server_name = server.name.clone().unwrap_or_else(|| server.id.clone());
        let initialization_options = server
            .initialization_options
            .as_ref()
            .map(yaml_to_json)
            .transpose()?;
        let client = Arc::new(Self {
            server_id: server.id.clone(),
            server_name,
            root: root.to_path_buf(),
            root_uri: root_uri.clone(),
            initialization_options,
            sender: tx.clone(),
            pending: DashMap::new(),
            diagnostics: DashMap::new(),
            diagnostics_tx: diag_tx,
            next_id: AtomicU64::new(1),
            last_used: AtomicU64::new(now_ts_u64()),
            alive: AtomicBool::new(true),
            file_versions: Mutex::new(HashMap::new()),
            process: Mutex::new(child),
        });

        let client_writer = Arc::downgrade(&client);
        tokio::spawn(async move {
            let mut writer = stdin;
            while let Some(message) = rx.recv().await {
                let payload = message.payload;
                if send_message(&mut writer, &payload).await.is_err() {
                    if let Some(client) = client_writer.upgrade() {
                        client.alive.store(false, Ordering::Relaxed);
                    }
                    break;
                }
            }
        });

        let client_reader = Arc::downgrade(&client);
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                let Some(message) = read_message(&mut reader).await else {
                    if let Some(client) = client_reader.upgrade() {
                        client.alive.store(false, Ordering::Relaxed);
                    }
                    break;
                };
                if let Some(client) = client_reader.upgrade() {
                    if let Err(err) = client.handle_incoming(message).await {
                        warn!("LSP incoming message error: {err}");
                    }
                } else {
                    break;
                }
            }
        });

        client.initialize(timeout_s).await?;
        Ok(client)
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    fn touch(&self) {
        self.last_used.store(now_ts_u64(), Ordering::Relaxed);
    }

    fn last_used_at(&self) -> f64 {
        self.last_used.load(Ordering::Relaxed) as f64
    }

    pub(crate) fn server_id(&self) -> &str {
        &self.server_id
    }

    pub(crate) fn server_name(&self) -> &str {
        &self.server_name
    }

    async fn initialize(&self, timeout_s: u64) -> Result<()> {
        let init_params = json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri,
            "workspaceFolders": [{
                "name": "workspace",
                "uri": self.root_uri,
            }],
            "initializationOptions": self.initialization_options.clone().unwrap_or_else(|| json!({})),
            "capabilities": {
                "window": {
                    "workDoneProgress": true
                },
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "didChangeWatchedFiles": { "dynamicRegistration": true }
                },
                "textDocument": {
                    "synchronization": {
                        "didOpen": true,
                        "didChange": true
                    },
                    "publishDiagnostics": {
                        "versionSupport": true
                    }
                }
            }
        });
        let response = self
            .send_request("initialize", init_params, timeout_s)
            .await?;
        let _ = response;
        self.send_notification("initialized", json!({})).await?;
        if let Some(init) = self.initialization_options.clone() {
            self.send_notification(
                "workspace/didChangeConfiguration",
                json!({ "settings": init }),
            )
            .await?;
        }
        Ok(())
    }

    async fn open_file(&self, path: &Path, notify_save: bool) -> Result<()> {
        let content = tokio::fs::read(path).await?;
        let text = String::from_utf8_lossy(&content).to_string();
        let language_id = detect_language_id(path);
        let uri = path_to_uri(path)?;
        let mut versions = self.file_versions.lock().await;
        if let Some(version) = versions.get_mut(path) {
            let next_version = version.saturating_add(1);
            *version = next_version;
            self.send_notification(
                "workspace/didChangeWatchedFiles",
                json!({ "changes": [{ "uri": uri, "type": 2 }] }),
            )
            .await?;
            self.send_notification(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": next_version },
                    "contentChanges": [{ "text": text }]
                }),
            )
            .await?;
            if notify_save {
                self.send_notification(
                    "textDocument/didSave",
                    json!({
                        "textDocument": { "uri": uri },
                        "text": text
                    }),
                )
                .await?;
            }
            return Ok(());
        }
        versions.insert(path.to_path_buf(), 0);
        self.send_notification(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": [{ "uri": uri, "type": 1 }] }),
        )
        .await?;
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 0,
                    "text": text
                }
            }),
        )
        .await?;
        if notify_save {
            self.send_notification(
                "textDocument/didSave",
                json!({
                    "textDocument": { "uri": uri },
                    "text": text
                }),
            )
            .await?;
        }
        Ok(())
    }

    async fn wait_for_diagnostics(&self, path: &Path, debounce_ms: u64, timeout_ms: u64) {
        let target = normalize_target_path(path);
        let mut rx = self.diagnostics_tx.subscribe();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let Ok(Ok(updated)) = timeout(remaining, rx.recv()).await else {
                break;
            };
            if normalize_path_for_compare(&updated) != normalize_path_for_compare(&target) {
                continue;
            }
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return;
                }
                let wait = Duration::from_millis(debounce_ms).min(remaining);
                let sleep_timer = sleep(wait);
                tokio::pin!(sleep_timer);
                tokio::select! {
                    _ = &mut sleep_timer => return,
                    recv = rx.recv() => {
                        if let Ok(updated) = recv {
                            if normalize_path_for_compare(&updated) == normalize_path_for_compare(&target) {
                                continue;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn send_notification(&self, method: &str, params: Value) -> Result<()> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.sender
            .send(OutgoingMessage { payload })
            .await
            .map_err(|_| anyhow!("LSP notification channel closed"))?;
        Ok(())
    }

    async fn send_request(&self, method: &str, params: Value, timeout_s: u64) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);
        self.sender
            .send(OutgoingMessage { payload })
            .await
            .map_err(|_| anyhow!("LSP request channel closed"))?;
        let timeout_s = if timeout_s == 0 {
            DEFAULT_TIMEOUT_S
        } else {
            timeout_s
        };
        match timeout(Duration::from_secs(timeout_s), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(anyhow!("LSP request cancelled")),
            Err(_) => {
                self.pending.remove(&id);
                Err(anyhow!("LSP request timeout"))
            }
        }
    }

    pub(crate) async fn request(
        &self,
        method: &str,
        params: Value,
        timeout_s: u64,
    ) -> Result<Value> {
        self.send_request(method, params, timeout_s).await
    }

    async fn respond(&self, id: Value, result: Value) -> Result<()> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        self.sender
            .send(OutgoingMessage { payload })
            .await
            .map_err(|_| anyhow!("LSP response channel closed"))?;
        Ok(())
    }

    async fn handle_incoming(&self, message: Value) -> Result<()> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id_value = message.get("id").cloned();
        if !method.is_empty() {
            if method == "textDocument/publishDiagnostics" {
                self.handle_diagnostics(&message).await;
                return Ok(());
            }
            if let Some(id) = id_value {
                let result = self.handle_request(method).await;
                let _ = self.respond(id, result).await;
            }
            return Ok(());
        }
        if let Some(id) = id_value {
            let id_num = parse_id(&id);
            if let Some(id_num) = id_num {
                if let Some((_, sender)) = self.pending.remove(&id_num) {
                    if let Some(error) = message.get("error") {
                        let _ = sender.send(Err(anyhow!("LSP error: {error}")));
                    } else {
                        let result = message.get("result").cloned().unwrap_or(Value::Null);
                        let _ = sender.send(Ok(result));
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_request(&self, method: &str) -> Value {
        match method {
            "workspace/configuration" => {
                let settings = self
                    .initialization_options
                    .clone()
                    .unwrap_or_else(|| json!({}));
                json!([settings])
            }
            "workspace/workspaceFolders" => json!([{
                "name": "workspace",
                "uri": self.root_uri
            }]),
            "client/registerCapability" | "client/unregisterCapability" => Value::Null,
            "window/workDoneProgress/create" => Value::Null,
            _ => Value::Null,
        }
    }

    async fn handle_diagnostics(&self, message: &Value) {
        let Some(params) = message.get("params") else {
            return;
        };
        let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
        let Ok(path) = uri_to_path(uri) else {
            return;
        };
        let diagnostics = params.get("diagnostics").cloned().unwrap_or(Value::Null);
        let parsed: Vec<LspDiagnostic> = serde_json::from_value(diagnostics).unwrap_or_default();
        let normalized = normalize_target_path(&path);
        self.diagnostics.insert(normalized.clone(), parsed);
        let _ = self.diagnostics_tx.send(normalized);
    }

    async fn shutdown(&self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.send_request("shutdown", json!({}), 5).await;
        let _ = self.send_notification("exit", json!({})).await;
        let mut guard = self.process.lock().await;
        let _ = guard.kill().await;
    }
}

fn resolve_servers_for_file(config: &Config, file_path: &Path) -> Vec<LspServerConfig> {
    let extension = file_extension(file_path);
    config
        .lsp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .filter(|server| {
            if server.extensions.is_empty() {
                return true;
            }
            server
                .extensions
                .iter()
                .any(|ext| normalize_extension(ext) == extension)
        })
        .cloned()
        .collect()
}

fn resolve_root_for_file(
    user_root: &Path,
    file_path: &Path,
    markers: &[String],
) -> Option<PathBuf> {
    let mut current = if file_path.is_dir() {
        file_path
    } else {
        file_path.parent()?
    };
    let root_normalized = normalize_target_path(user_root);
    let root_compare = normalize_path_for_compare(&root_normalized);
    if markers.is_empty() {
        return Some(root_normalized);
    }
    loop {
        for marker in markers {
            let candidate = current.join(marker);
            if candidate.exists() {
                return Some(current.to_path_buf());
            }
        }
        let current_compare = normalize_path_for_compare(current);
        if current_compare == root_compare {
            break;
        }
        current = current.parent()?;
    }
    Some(root_normalized)
}

fn normalize_command(command: &[String]) -> Result<Vec<String>> {
    if command.is_empty() {
        return Err(anyhow!("LSP command is empty"));
    }
    let normalized = command
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return Err(anyhow!("LSP command is empty"));
    }
    Ok(normalized)
}

fn file_extension(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_string();
    normalize_extension(&ext)
}

fn normalize_extension(value: &str) -> String {
    value.trim().trim_start_matches('.').to_lowercase()
}

fn detect_language_id(path: &Path) -> String {
    match file_extension(path).as_str() {
        "rs" => "rust",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" => "javascript",
        "jsx" => "javascriptreact",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "json" => "json",
        "jsonc" => "jsonc",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "less" => "less",
        "md" | "markdown" => "markdown",
        "sh" | "bash" | "zsh" => "shellscript",
        "dockerfile" => "dockerfile",
        "xml" => "xml",
        "sql" => "sql",
        "c" => "c",
        "cpp" | "cc" | "cxx" => "cpp",
        "h" => "c",
        "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "lua" => "lua",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" => "kotlin",
        _ => "plaintext",
    }
    .to_string()
}

fn resolve_timeout_s(config: &Config) -> u64 {
    if config.lsp.timeout_s == 0 {
        DEFAULT_TIMEOUT_S
    } else {
        config.lsp.timeout_s
    }
}

fn resolve_diagnostics_debounce_ms(config: &Config) -> u64 {
    if config.lsp.diagnostics_debounce_ms == 0 {
        DEFAULT_DIAGNOSTICS_DEBOUNCE_MS
    } else {
        config.lsp.diagnostics_debounce_ms
    }
}

fn path_to_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("invalid file path"))
}

fn uri_to_path(uri: &str) -> Result<PathBuf> {
    let parsed = Url::parse(uri)?;
    parsed
        .to_file_path()
        .map_err(|_| anyhow!("invalid file uri"))
}

fn parse_id(value: &Value) -> Option<u64> {
    match value {
        Value::Number(num) => num.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn yaml_to_json(value: &serde_yaml::Value) -> Result<Value> {
    serde_json::to_value(value).map_err(|err| anyhow!("convert yaml to json failed: {err}"))
}

async fn send_message(writer: &mut tokio::process::ChildStdin, payload: &Value) -> Result<()> {
    let body = serde_json::to_string(payload)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.as_bytes().len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(body.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_message(reader: &mut BufReader<tokio::process::ChildStdout>) -> Option<Value> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let bytes = match reader.read_line(&mut line).await {
            Ok(value) => value,
            Err(err) => {
                warn!("LSP read header failed: {err}");
                return None;
            }
        };
        if bytes == 0 {
            return None;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            if let Ok(length) = value.trim().parse::<usize>() {
                content_length = Some(length);
            }
        }
    }
    let length = match content_length {
        Some(value) => value,
        None => {
            warn!("LSP message missing content-length");
            return None;
        }
    };
    let mut buffer = vec![0u8; length];
    if let Err(err) = reader.read_exact(&mut buffer).await {
        warn!("LSP read body failed: {err}");
        return None;
    }
    let text = String::from_utf8_lossy(&buffer);
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => Some(value),
        Err(err) => {
            warn!(
                "LSP message json parse failed: {err}, body={}",
                truncate_text(&text, 512)
            );
            None
        }
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn now_ts_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn truncate_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut output = text[..end].to_string();
    output.push_str("...");
    output
}
