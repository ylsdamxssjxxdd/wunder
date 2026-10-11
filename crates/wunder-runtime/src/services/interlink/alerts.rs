//! Governance alert hooks for the interlink surface (docs §9.4, §13.5 18/20).
//!
//! The hook is a bypass. Classification is an in-memory step on a row that is
//! already being built, and raising an alert is a non-blocking hand-off to one
//! background task: no request handler and no tunnel socket ever waits on it,
//! and no return value depends on it. With no pump task started, or with the
//! bounded queue full, the alert is only counted as dropped.
//!
//! Triggers: L3 execution, an approval rejection storm on one device (5 inside
//! 10 minutes) and a node secret replayed after its dual-key grace window.
//!
//! Payload red line (docs §9.3): an alert carries identifiers and statistics
//! only - user, device, command id, kind, level, result code, count, instant.
//! Every string goes through [`token`], which rejects anything containing a
//! space or a path separator, so a message body, a file body, a tool argument
//! or an absolute path cannot reach an alert even when it leaked into an audit
//! detail field.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use wunder_core::interlink::{
    command_level, AUDIT_APPROVAL_DECIDE, AUDIT_COMMAND_ACK, AUDIT_COMMAND_FINISH,
    AUDIT_COMMAND_ISSUE, APPROVAL_REJECTED,
};

use crate::core::blocking;
use crate::state::AppState;
use crate::storage::{InterlinkAuditRecord, InterlinkCommandRecord};

use super::audit;
use super::secret;

/// Storm window: this many rejections for one device inside it alerts (§9.4).
pub const REJECTION_WINDOW_S: f64 = 600.0;
pub const REJECTION_THRESHOLD: usize = 5;
/// Bucket width of the rejection ring; `BUCKETS` of them cover the window.
const BUCKET_S: f64 = 60.0;
const BUCKETS: usize = 10;
/// Devices tracked at once; the least recently seen one is evicted beyond it.
const TRACKED_DEVICES_MAX: usize = 512;
/// Cooldown of one alert key; it is also what bounds the dedupe map.
const ALERT_COOLDOWN_S: f64 = 600.0;
const TRACKED_KEYS_MAX: usize = 1_024;
/// Hand-off queue between the detecting task and the pump (§10.1).
pub const QUEUE_CAPACITY: usize = 64;
/// Env var holding the webhook signing secret (docs §9.4). Deliberately not a
/// config key: config is echoed back to the 舰桥.
pub const ALERT_WEBHOOK_SECRET_ENV: &str = "WUNDER_ALERT_WEBHOOK_SECRET";
const WEBHOOK_TIMEOUT_S: u64 = 5;
/// A failed delivery is retried at most this often, then counted (§9.4).
const WEBHOOK_RETRIES: usize = 1;
/// Longest identifier copied into a payload.
const TOKEN_MAX_CHARS: usize = 64;
/// Value of the `X-Interlink-Event` header on every webhook delivery.
const WEBHOOK_EVENT: &str = "interlink.alert";

/// Audit action of the alert itself; it is deliberately not a trigger.
pub const ACTION_ALERT_RAISED: &str = "alert.raised";
/// Audit action of a refused tunnel handshake.
const ACTION_CHANNEL_REJECTED: &str = "channel.rejected";
/// Handshake refusal for a secret version past its grace window (acceptance 20
/// asks for reject *and* alert); the tunnel handler writes it as the reason.
pub const REASON_SECRET_STALE_VERSION: &str = "secret_stale_version";

pub const TRIGGER_L3_EXECUTION: &str = "l3_execution";
pub const TRIGGER_REJECTION_STORM: &str = "rejection_storm";
pub const TRIGGER_SECRET_STALE: &str = "secret_stale_version";

/// One governance alert. The field set is fixed by the §9.3 red line.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub trigger: String,
    pub user_id: String,
    pub device_id: Option<String>,
    pub command_id: Option<String>,
    pub kind: Option<String>,
    pub level: Option<String>,
    pub result: Option<String>,
    /// Statistic: rejections inside the window for a storm, else 1.
    pub count: usize,
    pub raised_at: f64,
}

impl Alert {
    /// Webhook body and audit detail of this alert: identifiers only.
    pub fn payload(&self) -> Value {
        let mut map = Map::new();
        insert(&mut map, "trigger", Some(self.trigger.as_str()));
        insert(&mut map, "user_id", Some(self.user_id.as_str()));
        insert(&mut map, "device_id", self.device_id.as_deref());
        insert(&mut map, "command_id", self.command_id.as_deref());
        insert(&mut map, "kind", self.kind.as_deref());
        insert(&mut map, "level", self.level.as_deref());
        insert(&mut map, "result", self.result.as_deref());
        map.insert("count".to_string(), Value::from(self.count as i64));
        map.insert("raised_at".to_string(), Value::from(self.raised_at));
        Value::Object(map)
    }

    pub fn payload_json(&self) -> String {
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| "{}".to_string())
    }
}

fn insert(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
}

/// Keep only short identifier-shaped scalars. Anything that can hold prose or a
/// path (a space, `/` or `\`) is dropped instead of truncated.
fn token(value: Option<&Value>) -> Option<String> {
    let raw = match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => return None,
    };
    let allowed: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
        .collect();
    if allowed.is_empty() || allowed.len() != raw.len() || allowed.len() > TOKEN_MAX_CHARS {
        return None;
    }
    Some(allowed)
}

fn token_of(value: Option<&str>) -> Option<String> {
    value.and_then(|value| token(Some(&Value::String(value.to_string()))))
}

fn ident(value: &str) -> String {
    token_of(Some(value)).unwrap_or_default()
}

fn detail(row: &InterlinkAuditRecord) -> Value {
    row.detail_digest
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .unwrap_or(Value::Null)
}

/// `device:<id>` -> `<id>`; the cloud node has no device to alert about.
fn device_of(node: Option<&str>) -> Option<String> {
    let device = node?.strip_prefix("device:")?.trim();
    token_of(Some(device))
}

/// Sliding rejection counters, one fixed ring per device.
#[derive(Debug)]
struct RejectionWindow {
    /// `(absolute bucket index, count)`; a stale bucket is reused in place.
    buckets: [(i64, u32); BUCKETS],
    last_seen_at: f64,
}

impl RejectionWindow {
    fn new(now: f64) -> Self {
        Self {
            buckets: [(0, 0); BUCKETS],
            last_seen_at: now,
        }
    }

    /// Record one rejection and return how many fall in the window ending at
    /// `now`. Fixed buckets instead of a per-device event list keep a storm
    /// cheap to track and impossible to grow without bound.
    fn note(&mut self, now: f64) -> usize {
        self.last_seen_at = now;
        let bucket = (now / BUCKET_S).floor() as i64;
        let slot = bucket.rem_euclid(BUCKETS as i64) as usize;
        let (recorded, count) = self.buckets[slot];
        self.buckets[slot] = (
            bucket,
            if recorded == bucket {
                count.saturating_add(1)
            } else {
                1
            },
        );
        let oldest = bucket - BUCKETS as i64 + 1;
        (0..BUCKETS)
            .map(|index| self.buckets[index])
            .filter(|(recorded, _)| *recorded >= oldest && *recorded <= bucket)
            .map(|(_, count)| count as usize)
            .sum()
    }

    fn is_expired(&self, now: f64) -> bool {
        now - self.last_seen_at > REJECTION_WINDOW_S
    }
}

/// Detection state: two bounded maps, pruned on the janitor cadence.
#[derive(Default)]
struct Detection {
    rejections: HashMap<String, RejectionWindow>,
    /// dedupe key -> instant until which the same alert stays suppressed.
    raised: HashMap<String, f64>,
}

impl Detection {
    fn note_rejection(&mut self, device: &str, now: f64) -> usize {
        if !self.rejections.contains_key(device) {
            if self.rejections.len() >= TRACKED_DEVICES_MAX {
                self.evict_rejections(now);
            }
            if self.rejections.len() >= TRACKED_DEVICES_MAX {
                // Unreachable with a non-zero cap; a device flood then simply
                // stops being tracked instead of resizing the map.
                return 0;
            }
            self.rejections
                .insert(device.to_string(), RejectionWindow::new(now));
        }
        match self.rejections.get_mut(device) {
            Some(window) => window.note(now),
            None => 0,
        }
    }

    fn evict_rejections(&mut self, now: f64) {
        self.rejections.retain(|_, window| !window.is_expired(now));
        while self.rejections.len() >= TRACKED_DEVICES_MAX {
            let oldest = self
                .rejections
                .iter()
                .min_by(|(_, left), (_, right)| {
                    left.last_seen_at
                        .partial_cmp(&right.last_seen_at)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(device, _)| device.clone());
            match oldest {
                Some(device) => {
                    self.rejections.remove(&device);
                }
                None => break,
            }
        }
    }

    /// True when this key has not alerted yet inside its cooldown.
    fn claim(&mut self, key: &str, now: f64, cooldown_s: f64) -> bool {
        self.raised.retain(|_, until| *until > now);
        if self.raised.get(key).is_some_and(|until| *until > now) {
            return false;
        }
        if self.raised.len() >= TRACKED_KEYS_MAX {
            // Cooldowns are uniform, so the earliest deadline goes first.
            if let Some(key) = self
                .raised
                .iter()
                .min_by(|(_, left), (_, right)| {
                    left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(key, _)| key.clone())
            {
                self.raised.remove(&key);
            }
        }
        self.raised.insert(key.to_string(), now + cooldown_s);
        true
    }

    fn prune(&mut self, now: f64) {
        self.raised.retain(|_, until| *until > now);
        self.rejections.retain(|_, window| !window.is_expired(now));
    }
}

static DETECTION: OnceLock<Mutex<Detection>> = OnceLock::new();
static SENDER: OnceLock<Mutex<Option<mpsc::Sender<Alert>>>> = OnceLock::new();
static PUMP_RUNNING: AtomicBool = AtomicBool::new(false);
static RAISED: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static DELIVERED: AtomicU64 = AtomicU64::new(0);
static WEBHOOK_FAILURES: AtomicU64 = AtomicU64::new(0);

fn detection() -> &'static Mutex<Detection> {
    DETECTION.get_or_init(|| Mutex::new(Detection::default()))
}

fn sender_slot() -> &'static Mutex<Option<mpsc::Sender<Alert>>> {
    SENDER.get_or_init(|| Mutex::new(None))
}

/// Observe one audit row. `audit::record` calls it, so every writer of the
/// interlink trail feeds the hook without touching its own call site.
pub fn observe(row: &InterlinkAuditRecord) {
    let alert = match detection().lock() {
        Ok(mut state) => detect(&mut state, row),
        Err(_) => None,
    };
    if let Some(alert) = alert {
        raise(alert);
    }
}

/// L3 signal for a command whose audit entries carry no kind: the ledger row is
/// the only place the kind is known. Bypass contract as [`observe`].
pub fn note_command(record: &InterlinkCommandRecord, result: Option<&str>) {
    let kind = token_of(Some(&record.kind)).filter(|kind| command_level(kind) == "L3");
    let command_id = token_of(Some(&record.command_id));
    let alert = match (kind, command_id) {
        (Some(kind), Some(command_id)) => {
            let mut state = match detection().lock() {
                Ok(state) => state,
                Err(_) => return,
            };
            if state.claim(&format!("l3|{command_id}"), record.created_at, ALERT_COOLDOWN_S) {
                Some(execution_alert(
                    &record.actor_user_id,
                    Some(record.to_node.as_str()),
                    command_id,
                    kind,
                    token_of(result),
                    record.created_at,
                ))
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(alert) = alert {
        raise(alert);
    }
}

/// One step of the detection state machine. Pure apart from `state`, so every
/// threshold is testable without the process global.
fn detect(state: &mut Detection, row: &InterlinkAuditRecord) -> Option<Alert> {
    let parsed = detail(row);
    match row.action.as_str() {
        // Both decision sources land on this one action: the cloud approval
        // endpoint and the node's own rejection reported through its ack, so a
        // storm is counted on a single chain instead of two parallel ones.
        AUDIT_APPROVAL_DECIDE
            if token(parsed.get("state")).as_deref() == Some(APPROVAL_REJECTED) =>
        {
            let device = device_of(row.to_node.as_deref())?;
            let count = state.note_rejection(&device, row.created_at);
            if count < REJECTION_THRESHOLD {
                return None;
            }
            if !state.claim(&format!("storm|{device}"), row.created_at, ALERT_COOLDOWN_S) {
                return None;
            }
            let kind = token(parsed.get("kind"));
            Some(Alert {
                trigger: TRIGGER_REJECTION_STORM.to_string(),
                user_id: ident(&row.actor),
                device_id: Some(device),
                command_id: token_of(row.command_id.as_deref()),
                kind: kind.clone(),
                level: kind.map(|kind| command_level(&kind).to_string()),
                result: Some(APPROVAL_REJECTED.to_string()),
                count,
                raised_at: row.created_at,
            })
        }
        ACTION_CHANNEL_REJECTED
            if token(parsed.get("reason")).as_deref() == Some(REASON_SECRET_STALE_VERSION) =>
        {
            let device = device_of(row.to_node.as_deref())?;
            if !state.claim(&format!("stale|{device}"), row.created_at, ALERT_COOLDOWN_S) {
                return None;
            }
            Some(Alert {
                trigger: TRIGGER_SECRET_STALE.to_string(),
                user_id: ident(&row.actor),
                device_id: Some(device),
                command_id: token_of(row.command_id.as_deref()),
                kind: None,
                level: None,
                result: Some(REASON_SECRET_STALE_VERSION.to_string()),
                count: 1,
                raised_at: row.created_at,
            })
        }
        AUDIT_COMMAND_ISSUE | AUDIT_COMMAND_ACK | AUDIT_COMMAND_FINISH => {
            let kind = token(parsed.get("kind")).filter(|kind| command_level(kind) == "L3")?;
            let command_id = token_of(row.command_id.as_deref())?;
            if !state.claim(&format!("l3|{command_id}"), row.created_at, ALERT_COOLDOWN_S) {
                return None;
            }
            let result = token(parsed.get("result"))
                .or_else(|| token(parsed.get("status")))
                .or_else(|| token(parsed.get("code")));
            Some(execution_alert(
                &row.actor,
                row.to_node.as_deref(),
                command_id,
                kind,
                result,
                row.created_at,
            ))
        }
        _ => None,
    }
}

fn execution_alert(
    user_id: &str,
    to_node: Option<&str>,
    command_id: String,
    kind: String,
    result: Option<String>,
    now: f64,
) -> Alert {
    Alert {
        trigger: TRIGGER_L3_EXECUTION.to_string(),
        user_id: ident(user_id),
        device_id: device_of(to_node),
        command_id: Some(command_id),
        kind: Some(kind),
        level: Some("L3".to_string()),
        result,
        count: 1,
        raised_at: now,
    }
}

/// Hand the alert to the pump; `try_send` keeps the caller unblocked.
fn raise(alert: Alert) {
    RAISED.fetch_add(1, Ordering::Relaxed);
    let slot = match sender_slot().lock() {
        Ok(slot) => slot,
        Err(_) => {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    match slot.as_ref() {
        Some(sender) if sender.try_send(alert).is_ok() => {}
        _ => {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Drop detection state that aged out. The janitor calls this on its own 30s
/// cadence, so the bounded maps only ever hold recent activity.
pub fn prune(now: f64) {
    if let Ok(mut state) = detection().lock() {
        state.prune(now);
    }
}

/// Start the single alert pump. It owns the audit write and the optional
/// webhook delivery, so nothing on the detecting side performs IO. One pump
/// means one in-flight delivery: no concurrency, no retry storm and no backlog
/// beyond the bounded queue.
pub fn spawn(state: Arc<AppState>) {
    if PUMP_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let (sender, mut receiver) = mpsc::channel(QUEUE_CAPACITY);
    if let Ok(mut slot) = sender_slot().lock() {
        *slot = Some(sender);
    }
    let handle = tokio::runtime::Handle::current();
    handle.spawn(async move {
        while let Some(alert) = receiver.recv().await {
            // One config read per alert: the target URL and its signing secret
            // always come from the same snapshot.
            let (webhook, webhook_secret) = webhook_target(&state).await;
            write_audit(&state, &alert).await;
            if !webhook.is_empty() {
                deliver(&webhook, &webhook_secret, &alert).await;
            }
        }
        if let Ok(mut slot) = sender_slot().lock() {
            *slot = None;
        }
        PUMP_RUNNING.store(false, Ordering::SeqCst);
    });
}

pub fn is_running() -> bool {
    PUMP_RUNNING.load(Ordering::SeqCst)
}

/// Webhook target: `(url, signing secret)`, empty url when nothing is configured
/// or the scheme is not http(s).
///
/// The URL is ordinary config; the secret is env-only (`WUNDER_ALERT_WEBHOOK_SECRET`)
/// on purpose: the whole config is echoed back to the 舰桥 and pulled by
/// diagnostics, so a signing key in it would be readable by anyone who can see
/// the settings page.
async fn webhook_target(state: &AppState) -> (String, String) {
    let config = state.config_store.get().await;
    let url = config.interlink.alert_webhook.trim().to_string();
    drop(config);
    let secret = std::env::var(ALERT_WEBHOOK_SECRET_ENV)
        .unwrap_or_default()
        .trim()
        .to_string();
    // Only http(s) endpoints: a mistyped value must not turn the hook into a
    // local file read or a probe of an unexpected scheme.
    if url.starts_with("http://") || url.starts_with("https://") {
        (url, secret)
    } else {
        (String::new(), String::new())
    }
}

async fn write_audit(state: &AppState, alert: &Alert) {
    let payload = alert.payload();
    let mut detail: Vec<(&str, Value)> = vec![
        ("trigger", payload.get("trigger").cloned().unwrap_or(Value::Null)),
        ("count", payload.get("count").cloned().unwrap_or(Value::Null)),
    ];
    for key in ["kind", "level", "result"] {
        if let Some(value) = payload.get(key) {
            detail.push((key, value.clone()));
        }
    }
    let device = alert.device_id.as_deref().map(|device| format!("device:{device}"));
    let row = audit::record(
        ACTION_ALERT_RAISED,
        if alert.user_id.is_empty() { "system" } else { &alert.user_id },
        None,
        device.as_deref(),
        alert.command_id.as_deref(),
        None,
        detail,
    );
    let storage = state.storage.clone();
    let _ = blocking::run_db("interlink.alert.write", move || storage.insert_interlink_audit(&row))
        .await;
}

async fn deliver(url: &str, secret: &str, alert: &Alert) {
    // Sign the exact bytes that go on the wire: serialize once and send the
    // same string instead of letting reqwest re-serialize the payload.
    let body = alert.payload_json();
    let ts = unix_timestamp();
    for attempt in 0..=WEBHOOK_RETRIES {
        let mut request = http_client()
            .post(url)
            .timeout(Duration::from_secs(WEBHOOK_TIMEOUT_S))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header("X-Interlink-Event", WEBHOOK_EVENT)
            .body(body.clone());
        for (name, value) in signature_headers(secret, ts, &body) {
            request = request.header(name, value);
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                DELIVERED.fetch_add(1, Ordering::Relaxed);
                return;
            }
            Ok(_) | Err(_) => {
                if attempt == WEBHOOK_RETRIES {
                    WEBHOOK_FAILURES.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// `X-Interlink-Signature` value: `sha256=HMAC_SHA256(secret, "<ts>.<body>")`.
/// Pure so the convention is testable without the process global; the salted
/// input reuses the interlink HMAC helper (§9.4).
fn webhook_signature(secret: &str, ts: i64, body: &str) -> String {
    format!(
        "sha256={}",
        secret::hmac_hex(secret.as_bytes(), format!("{ts}.{body}").as_bytes())
    )
}

/// Signature headers of one delivery; empty when no secret is configured, so
/// the unkeyed behavior stays what it was. The timestamp is inside the MAC
/// input so a captured request cannot be replayed past the receiver's window.
fn signature_headers(secret: &str, ts: i64, body: &str) -> Vec<(&'static str, String)> {
    if secret.is_empty() {
        return Vec::new();
    }
    vec![
        ("X-Interlink-Timestamp", ts.to_string()),
        ("X-Interlink-Signature", webhook_signature(secret, ts, body)),
    ]
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(WEBHOOK_TIMEOUT_S))
            .timeout(Duration::from_secs(WEBHOOK_TIMEOUT_S))
            .build()
            .expect("build interlink alert http client")
    })
}

/// Runtime observability for the bridge monitoring panel (docs §9.4).
pub fn stats() -> Value {
    let tracked = detection()
        .lock()
        .map(|state| state.rejections.len() + state.raised.len())
        .unwrap_or(0);
    json!({
        "raised": RAISED.load(Ordering::Relaxed),
        "dropped": DROPPED.load(Ordering::Relaxed),
        "delivered": DELIVERED.load(Ordering::Relaxed),
        "webhook_failures": WEBHOOK_FAILURES.load(Ordering::Relaxed),
        "queue_capacity": QUEUE_CAPACITY,
        "tracked": tracked,
        "pump_running": is_running(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit_row(
        action: &str,
        device: &str,
        command_id: &str,
        detail: &str,
        at: f64,
    ) -> InterlinkAuditRecord {
        InterlinkAuditRecord {
            seq: 0,
            command_id: (!command_id.is_empty()).then(|| command_id.to_string()),
            approval_id: Some("apv_1".to_string()),
            actor: "u_1".to_string(),
            from_node: Some("web".to_string()),
            to_node: Some(format!("device:{device}")),
            action: action.to_string(),
            detail_digest: Some(detail.to_string()),
            created_at: at,
        }
    }

    fn rejection(device: &str, at: f64) -> InterlinkAuditRecord {
        audit_row(
            AUDIT_APPROVAL_DECIDE,
            device,
            "cmd_r",
            r#"{"state":"rejected"}"#,
            at,
        )
    }

    fn command(detail: &str, at: f64) -> InterlinkAuditRecord {
        audit_row(AUDIT_COMMAND_FINISH, "dev_cmd", "cmd_x", detail, at)
    }

    #[test]
    fn storm_alerts_on_the_fifth_rejection_of_one_device() {
        let mut state = Detection::default();
        let base = 1_000_000.0;
        for index in 1..REJECTION_THRESHOLD {
            assert!(
                detect(&mut state, &rejection("dev_a", base + index as f64)).is_none(),
                "{index} rejections must stay below the threshold"
            );
        }
        let alert = detect(&mut state, &rejection("dev_a", base + REJECTION_THRESHOLD as f64))
            .expect("the fifth rejection alerts");
        assert_eq!(alert.trigger, TRIGGER_REJECTION_STORM);
        assert_eq!(alert.count, REJECTION_THRESHOLD);
        assert_eq!(alert.device_id.as_deref(), Some("dev_a"));
        assert_eq!(alert.user_id, "u_1");
        assert_eq!(alert.result.as_deref(), Some(APPROVAL_REJECTED));
        // The burst keeps running, but one window yields at most one alert.
        for index in 6..10 {
            assert!(detect(&mut state, &rejection("dev_a", base + index as f64)).is_none());
        }
    }

    #[test]
    fn storms_are_tracked_per_device() {
        let mut state = Detection::default();
        let base = 1_100_000.0;
        for index in 0..4 {
            assert!(detect(&mut state, &rejection("dev_c", base + index as f64)).is_none());
            assert!(detect(&mut state, &rejection("dev_d", base + index as f64)).is_none());
        }
        assert!(detect(&mut state, &rejection("dev_c", base + 40.0)).is_some());
        assert!(detect(&mut state, &rejection("dev_d", base + 40.0)).is_some());
    }

    #[test]
    fn rejection_window_slides_and_forgets() {
        let mut state = Detection::default();
        let base = 2_000_000.0;
        for index in 0..4 {
            assert_eq!(
                state.note_rejection("dev_s", base + index as f64 * 60.0),
                index + 1
            );
        }
        // One full window later nothing of that burst is counted any more.
        assert_eq!(state.note_rejection("dev_s", base + 4.0 * REJECTION_WINDOW_S), 1);
    }

    #[test]
    fn rejection_ring_reuses_the_oldest_bucket() {
        let mut window = RejectionWindow::new(0.0);
        assert_eq!(window.note(0.0), 1);
        assert_eq!(window.note(59.0), 2);
        assert_eq!(window.note(600.0), 1);
        assert_eq!(window.note(660.0), 2);
        assert!(!window.is_expired(1_200.0));
        assert!(window.is_expired(1_261.0));
    }

    #[test]
    fn l3_execution_alerts_once_per_command_and_ignores_lower_levels() {
        let mut state = Detection::default();
        let base = 3_000_000.0;
        assert!(
            detect(&mut state, &command(r#"{"kind":"workspace.read"}"#, base)).is_none(),
            "an L0 command must not alert"
        );
        assert!(
            detect(&mut state, &command(r#"{"kind":"workspace.write"}"#, base + 1.0)).is_none(),
            "an L2 command must not alert"
        );
        let alert = detect(&mut state, &command(r#"{"kind":"tool.exec","status":"succeeded"}"#, base + 2.0))
            .expect("L3 alerts");
        assert_eq!(alert.trigger, TRIGGER_L3_EXECUTION);
        assert_eq!(alert.kind.as_deref(), Some("tool.exec"));
        assert_eq!(alert.level.as_deref(), Some("L3"));
        assert_eq!(alert.result.as_deref(), Some("succeeded"));
        assert_eq!(alert.command_id.as_deref(), Some("cmd_x"));
        assert_eq!(alert.device_id.as_deref(), Some("dev_cmd"));
        // Whichever trail entry reaches the hook next, one command alerts once.
        let ack =
            audit_row(AUDIT_COMMAND_ACK, "dev_cmd", "cmd_x", r#"{"kind":"agent.spawn"}"#, base + 3.0);
        assert!(detect(&mut state, &ack).is_none());
        let other =
            audit_row(AUDIT_COMMAND_ISSUE, "dev_cmd", "cmd_y", r#"{"kind":"agent.spawn"}"#, base + 4.0);
        assert!(detect(&mut state, &other).is_some());
    }

    #[test]
    fn stale_secret_handshake_alerts_and_other_refusals_do_not() {
        let mut state = Detection::default();
        let base = 4_000_000.0;
        let mismatch = audit_row(
            ACTION_CHANNEL_REJECTED,
            "dev_k",
            "",
            r#"{"reason":"hmac_mismatch"}"#,
            base,
        );
        assert!(detect(&mut state, &mismatch).is_none());
        let stale = audit_row(
            ACTION_CHANNEL_REJECTED,
            "dev_k",
            "",
            r#"{"reason":"secret_stale_version"}"#,
            base + 1.0,
        );
        let alert = detect(&mut state, &stale).expect("acceptance 20: reject and alert");
        assert_eq!(alert.trigger, TRIGGER_SECRET_STALE);
        assert_eq!(alert.device_id.as_deref(), Some("dev_k"));
        assert_eq!(alert.result.as_deref(), Some(REASON_SECRET_STALE_VERSION));
        assert!(alert.command_id.is_none());
        // A repeated replay stays inside the cooldown.
        assert!(detect(&mut state, &stale).is_none());
    }

    #[test]
    fn payload_holds_identifiers_only() {
        let mut state = Detection::default();
        let alert = detect(
            &mut state,
            &command(r#"{"kind":"tool.exec","code":"NODE_OFFLINE"}"#, 5_000_000.0),
        )
        .expect("L3 alerts");
        assert_eq!(alert.result.as_deref(), Some("NODE_OFFLINE"));
        let text = alert.payload_json();
        for key in [
            "trigger",
            "user_id",
            "device_id",
            "command_id",
            "kind",
            "level",
            "result",
            "count",
            "raised_at",
        ] {
            assert!(text.contains(key), "{key} missing from {text}");
        }
        // The payload never carries the audit detail container or its columns.
        assert!(!text.contains("detail"), "{text}");
        assert!(!text.contains("digest"), "{text}");
        assert!(!text.contains("prompt"), "{text}");
        assert!(!text.contains("args"), "{text}");
    }

    #[test]
    fn payload_never_carries_bodies_or_absolute_paths() {
        let mut state = Detection::default();
        // A digest that leaked prose, a Windows path and a message body must
        // still produce an alert that carries none of it.
        let poisoned = command(
            r#"{"kind":"tool.exec","result":"C:\\Users\\one\\notes.txt","status":"please forward the whole report","message":"confidential body"}"#,
            6_000_000.0,
        );
        let alert = detect(&mut state, &poisoned).expect("L3 alerts");
        assert_eq!(alert.result, None, "path-like and prose results are dropped");
        let text = alert.payload_json();
        for forbidden in [
            "Users", "notes", "txt", "forward", "report", "confidential", "\\", "C:",
        ] {
            assert!(!text.contains(forbidden), "payload leaked {forbidden:?}: {text}");
        }
    }

    #[test]
    fn storm_payload_keeps_only_the_state_code() {
        let mut state = Detection::default();
        let base = 7_000_000.0;
        let leaked = r#"{"state":"rejected","prompt":"run /srv/data/export.sh now","args":"echo the key"}"#;
        let mut alerts = 0usize;
        for index in 0..REJECTION_THRESHOLD {
            let alert = detect(
                &mut state,
                &audit_row(AUDIT_APPROVAL_DECIDE, "dev_p", "cmd_p", leaked, base + index as f64),
            );
            if alert.is_some() {
                alerts += 1;
                let text = alert.expect("storm alert").payload_json();
                for forbidden in ["srv", "export", "echo", "key", "run", "/"] {
                    assert!(!text.contains(forbidden), "payload leaked {forbidden:?}: {text}");
                }
            }
        }
        assert_eq!(alerts, 1, "only the crossed threshold alerts");
    }

    #[test]
    fn alert_rows_never_trigger_the_hook_again() {
        // The pump writes `alert.raised` through the same trail, so the
        // classifier must not treat it as a signal or the hook would feed
        // itself.
        let mut state = Detection::default();
        let raised = audit_row(
            ACTION_ALERT_RAISED,
            "dev_r",
            "cmd_r",
            r#"{"state":"rejected","trigger":"rejection_storm","reason":"secret_stale_version","kind":"tool.exec"}"#,
            11_000_000.0,
        );
        assert!(detect(&mut state, &raised).is_none());
        assert!(state.rejections.is_empty());
        assert!(state.raised.is_empty());
    }

    #[test]
    fn token_filter_accepts_identifiers_and_rejects_prose() {
        assert_eq!(token(Some(&json!("tool.exec"))).as_deref(), Some("tool.exec"));
        assert_eq!(token(Some(&json!("shadow:full"))).as_deref(), Some("shadow:full"));
        assert_eq!(token(Some(&json!("apv-9_x1"))).as_deref(), Some("apv-9_x1"));
        assert_eq!(token(Some(&json!("notes/a.md"))), None, "paths are dropped");
        assert_eq!(token(Some(&json!("C:\\a.txt"))), None, "windows paths are dropped");
        assert_eq!(token(Some(&json!("say hello"))), None, "prose is dropped");
        assert_eq!(token(Some(&json!("x".repeat(TOKEN_MAX_CHARS + 1)))), None);
        assert_eq!(token(Some(&json!(42))).as_deref(), Some("42"));
        assert_eq!(token(Some(&json!({"a": 1}))), None);
        assert_eq!(token(None), None);
    }

    #[test]
    fn detection_maps_are_bounded() {
        let base = 8_000_000.0;
        let mut state = Detection::default();
        for index in 0..(TRACKED_DEVICES_MAX + 64) {
            state.note_rejection(&format!("dev_{index}"), base);
            assert!(state.rejections.len() <= TRACKED_DEVICES_MAX);
        }
        for index in 0..(TRACKED_KEYS_MAX + 64) {
            state.claim(&format!("l3|cmd_{index}"), base, ALERT_COOLDOWN_S);
            assert!(state.raised.len() <= TRACKED_KEYS_MAX);
        }
        state.prune(base + REJECTION_WINDOW_S + 1.0);
        assert!(state.rejections.is_empty());
        assert!(state.raised.is_empty());
    }

    #[test]
    fn dedupe_suppresses_only_inside_the_cooldown() {
        let base = 9_000_000.0;
        let mut state = Detection::default();
        assert!(state.claim("storm|dev_e", base, ALERT_COOLDOWN_S));
        assert!(!state.claim("storm|dev_e", base + 60.0, ALERT_COOLDOWN_S));
        assert!(state.claim("storm|dev_e", base + ALERT_COOLDOWN_S + 1.0, ALERT_COOLDOWN_S));
    }

    #[test]
    fn observe_without_a_pump_only_counts_the_alert() {
        // Unit tests never start the pump: the hand-off must stay harmless.
        let before = stats()["dropped"].as_i64().unwrap_or(0);
        for index in 0..REJECTION_THRESHOLD {
            observe(&rejection("dev_unspawned", 10_000_000.0 + index as f64));
        }
        let after = stats()["dropped"].as_i64().unwrap_or(0);
        assert!(after > before, "the alert was counted, not blocking");
        assert_eq!(stats()["queue_capacity"].as_i64(), Some(QUEUE_CAPACITY as i64));
        assert_eq!(stats()["pump_running"], Value::Bool(false));
    }

    #[test]
    fn stats_report_delivery_counters() {
        let value = stats();
        for key in ["raised", "dropped", "delivered", "webhook_failures", "tracked"] {
            assert!(value[key].is_number(), "{key} missing from {value}");
        }
    }

    #[test]
    fn webhook_signature_matches_a_fixed_vector() {
        // Key and body are meaningless placeholders; the digest is the
        // standard HMAC-SHA256 over "<ts>.<body>".
        let body = r#"{"trigger":"l3_execution"}"#;
        assert_eq!(
            webhook_signature("test-secret", 1_700_000_000, body),
            "sha256=42b2d0ecfc2535fb81db88221794fef1896cd42f86674b616fdf5710601e4c0b"
        );
        // Reproducible, and sensitive to every input.
        assert_eq!(
            webhook_signature("test-secret", 1_700_000_000, body),
            webhook_signature("test-secret", 1_700_000_000, body)
        );
        assert_ne!(
            webhook_signature("test-secret", 1_700_000_001, body),
            webhook_signature("test-secret", 1_700_000_000, body),
            "the timestamp must be part of the MAC input"
        );
        assert_ne!(
            webhook_signature("other-secret", 1_700_000_000, body),
            webhook_signature("test-secret", 1_700_000_000, body)
        );
        assert_ne!(
            webhook_signature("test-secret", 1_700_000_000, "{}"),
            webhook_signature("test-secret", 1_700_000_000, body)
        );
    }

    #[test]
    fn signature_headers_only_exist_with_a_secret() {
        let body = r#"{"trigger":"rejection_storm"}"#;
        assert!(
            signature_headers("", 1_700_000_000, body).is_empty(),
            "no secret keeps today's unsigned delivery"
        );
        let headers = signature_headers("test-secret", 1_700_000_000, body);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].0, "X-Interlink-Timestamp");
        assert_eq!(headers[0].1, "1700000000");
        assert_eq!(headers[1].0, "X-Interlink-Signature");
        assert_eq!(
            headers[1].1,
            webhook_signature("test-secret", 1_700_000_000, body)
        );
    }
}
