use super::support::{build_internal_channel_headers, channels_runtime_enabled};
use super::ChannelHub;
use crate::channels::types::{
    ChannelAccountConfig, ChannelMessage, FeishuConfig, QqBotConfig, WeixinConfig, XmppConfig,
};
use crate::channels::{feishu, qqbot, weixin, xmpp};
use crate::core::blocking;
use crate::core::long_task;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tokio::time::{sleep, Duration};
use tracing::debug;

const FEISHU_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 10;
const FEISHU_LONG_CONN_RETRY_BASE_S: u64 = 3;
const FEISHU_LONG_CONN_RETRY_MAX_S: u64 = 30;
const QQBOT_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 10;
const QQBOT_LONG_CONN_RETRY_BASE_S: u64 = 3;
const QQBOT_LONG_CONN_RETRY_MAX_S: u64 = 30;
const XMPP_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 10;
const XMPP_LONG_CONN_RETRY_BASE_S: u64 = 3;
const XMPP_LONG_CONN_RETRY_MAX_S: u64 = 30;
const WEIXIN_LONG_CONN_SUPERVISOR_INTERVAL_S: u64 = 2;
const WEIXIN_LONG_CONN_RETRY_BASE_MS: u64 = 800;

#[derive(Debug, Clone)]
struct FeishuLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: FeishuConfig,
}

#[derive(Debug, Clone)]
struct QqBotLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: QqBotConfig,
}

#[derive(Debug, Clone)]
struct XmppLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: XmppConfig,
}

#[derive(Debug, Clone)]
struct WeixinLongConnTarget {
    account_id: String,
    updated_at: f64,
    inbound_token: Option<String>,
    config: WeixinConfig,
}

impl FeishuLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

impl QqBotLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

impl XmppLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

impl WeixinLongConnTarget {
    fn task_key(&self) -> String {
        format!("{}:{:.3}", self.account_id, self.updated_at)
    }
}

impl ChannelHub {
    pub(super) async fn feishu_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(FEISHU_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_feishu_long_connection_targets().await {
                Ok(targets) => {
                    let mut desired_keys = HashSet::new();
                    for target in targets {
                        let task_key = target.task_key();
                        desired_keys.insert(task_key.clone());
                        if workers.contains_key(&task_key) {
                            continue;
                        }
                        let worker = self.clone();
                        workers.insert(
                            task_key,
                            long_task::spawn(
                                "channels.long_connection.feishu.worker",
                                async move {
                                    worker.feishu_long_connection_worker_loop(target).await;
                                },
                            ),
                        );
                    }

                    let stale_keys = workers
                        .keys()
                        .filter(|key| !desired_keys.contains(*key))
                        .cloned()
                        .collect::<Vec<_>>();
                    for task_key in stale_keys {
                        if let Some(handle) = workers.remove(&task_key) {
                            handle.abort();
                        }
                    }
                }
                Err(err) => {
                    self.record_runtime_warn(
                        feishu::FEISHU_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load feishu long connection targets failed: {err}"),
                    );
                    debug!("load feishu long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(FEISHU_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_feishu_long_connection_targets(&self) -> Result<Vec<FeishuLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts =
            blocking::run_db("channels.long_connection.feishu.list_accounts", move || {
                storage.list_channel_accounts(Some(feishu::FEISHU_CHANNEL), Some("active"))
            })
            .await?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(feishu_cfg) = account_cfg.feishu else {
                continue;
            };
            if !feishu::long_connection_enabled(&feishu_cfg) {
                continue;
            }
            if !feishu::has_long_connection_credentials(&feishu_cfg) {
                self.record_runtime_warn(
                    feishu::FEISHU_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip feishu long connection target without app credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
                    "skip feishu long connection target without app credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(FeishuLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: feishu_cfg,
            });
        }

        Ok(targets)
    }

    async fn feishu_long_connection_worker_loop(&self, target: FeishuLongConnTarget) {
        let mut retry_delay_s = FEISHU_LONG_CONN_RETRY_BASE_S;
        loop {
            let result = feishu::run_long_connection_session(&self.http, &target.config, {
                let worker = self.clone();
                let event_target = target.clone();
                move |payload| {
                    let worker = worker.clone();
                    let event_target = event_target.clone();
                    async move {
                        worker
                            .handle_feishu_long_connection_payload(&event_target, payload)
                            .await
                    }
                }
            })
            .await;

            match result {
                Ok(endpoint) => {
                    retry_delay_s = endpoint
                        .reconnect_interval_s
                        .clamp(FEISHU_LONG_CONN_RETRY_BASE_S, FEISHU_LONG_CONN_RETRY_MAX_S);
                    self.record_runtime_warn(
                        feishu::FEISHU_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_closed",
                        format!(
                            "feishu long connection closed: account_id={}, retry_in={}s",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "feishu long connection closed: account_id={}, retry_in={}s",
                        target.account_id, retry_delay_s
                    );
                }
                Err(err) => {
                    self.record_runtime_warn(
                        feishu::FEISHU_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_failed",
                        format!(
                            "feishu long connection failed: account_id={}, retry_in={}s, error={err}",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "feishu long connection failed: account_id={}, retry_in={}s, error={err}",
                        target.account_id, retry_delay_s
                    );
                    retry_delay_s = (retry_delay_s * 2).min(FEISHU_LONG_CONN_RETRY_MAX_S);
                }
            }

            sleep(Duration::from_secs(retry_delay_s)).await;
        }
    }

    async fn handle_feishu_long_connection_payload(
        &self,
        target: &FeishuLongConnTarget,
        payload: Value,
    ) -> Result<()> {
        let resolved_payload =
            feishu::decrypt_event_if_needed(payload, target.config.encrypt_key.as_deref())?;
        if !feishu::is_message_event(&resolved_payload) {
            return Ok(());
        }
        let messages =
            feishu::extract_inbound_messages(&resolved_payload, &target.account_id, Some("user"))?;
        if messages.is_empty() {
            return Ok(());
        }
        let headers = build_internal_channel_headers(target.inbound_token.as_deref())?;
        let result = self
            .enqueue_inbound(
                feishu::FEISHU_CHANNEL,
                &headers,
                messages,
                Some(resolved_payload),
            )
            .await?;
        if result.accepted == 0 {
            return Err(anyhow!(
                "feishu long connection inbound ignored: no message accepted"
            ));
        }
        Ok(())
    }

    pub(super) async fn qqbot_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(QQBOT_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_qqbot_long_connection_targets().await {
                Ok(targets) => {
                    let mut desired_keys = HashSet::new();
                    for target in targets {
                        let task_key = target.task_key();
                        desired_keys.insert(task_key.clone());
                        if workers.contains_key(&task_key) {
                            continue;
                        }
                        let worker = self.clone();
                        workers.insert(
                            task_key,
                            long_task::spawn("channels.long_connection.qqbot.worker", async move {
                                worker.qqbot_long_connection_worker_loop(target).await;
                            }),
                        );
                    }

                    let stale_keys = workers
                        .keys()
                        .filter(|key| !desired_keys.contains(*key))
                        .cloned()
                        .collect::<Vec<_>>();
                    for task_key in stale_keys {
                        if let Some(handle) = workers.remove(&task_key) {
                            handle.abort();
                        }
                    }
                }
                Err(err) => {
                    self.record_runtime_warn(
                        qqbot::QQBOT_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load qqbot long connection targets failed: {err}"),
                    );
                    debug!("load qqbot long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(QQBOT_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_qqbot_long_connection_targets(&self) -> Result<Vec<QqBotLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts =
            blocking::run_db("channels.long_connection.qqbot.list_accounts", move || {
                storage.list_channel_accounts(Some(qqbot::QQBOT_CHANNEL), Some("active"))
            })
            .await?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(qqbot_cfg) = account_cfg.qqbot else {
                continue;
            };
            if !qqbot::long_connection_enabled(&qqbot_cfg) {
                continue;
            }
            if !qqbot::has_long_connection_credentials(&qqbot_cfg) {
                self.record_runtime_warn(
                    qqbot::QQBOT_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip qqbot long connection target without credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
                    "skip qqbot long connection target without credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(QqBotLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: qqbot_cfg,
            });
        }

        Ok(targets)
    }

    async fn qqbot_long_connection_worker_loop(&self, target: QqBotLongConnTarget) {
        let intent_candidates = qqbot::resolve_long_connection_intent_candidates(&target.config);
        let mut intent_index = 0_usize;
        let mut retry_delay_s = QQBOT_LONG_CONN_RETRY_BASE_S;
        self.record_runtime_info(
            qqbot::QQBOT_CHANNEL,
            Some(&target.account_id),
            "long_connection_worker_started",
            format!(
                "qqbot long connection worker started: account_id={}, intent_candidates={:?}",
                target.account_id, intent_candidates
            ),
        );
        loop {
            let intents = intent_candidates
                .get(intent_index)
                .copied()
                .unwrap_or_else(|| qqbot::resolve_long_connection_intents(&target.config));
            self.record_runtime_info(
                qqbot::QQBOT_CHANNEL,
                Some(&target.account_id),
                "long_connection_connecting",
                format!(
                    "qqbot long connection connecting: account_id={}, intents={}, intent_level={}/{}",
                    target.account_id,
                    intents,
                    intent_index + 1,
                    intent_candidates.len()
                ),
            );

            let result = qqbot::run_long_connection_session_with_intents(
                &self.http,
                &target.config,
                intents,
                {
                    let worker = self.clone();
                    let event_target = target.clone();
                    move |payload| {
                        let worker = worker.clone();
                        let event_target = event_target.clone();
                        async move {
                            worker
                                .handle_qqbot_long_connection_payload(&event_target, payload)
                                .await
                        }
                    }
                },
            )
            .await;

            match result {
                Ok(()) => {
                    retry_delay_s = QQBOT_LONG_CONN_RETRY_BASE_S;
                    self.record_runtime_warn(
                        qqbot::QQBOT_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_closed",
                        format!(
                            "qqbot long connection closed: account_id={}, retry_in={}s",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "qqbot long connection closed: account_id={}, retry_in={}s",
                        target.account_id, retry_delay_s
                    );
                }
                Err(err) => {
                    let err_text = err.to_string();
                    let should_try_lower_intent = intent_candidates.len() > 1
                        && intent_index + 1 < intent_candidates.len()
                        && qqbot::should_try_lower_intent_after_error(err_text.as_str());
                    if should_try_lower_intent {
                        let previous_intents = intents;
                        intent_index += 1;
                        let next_intents = intent_candidates[intent_index];
                        retry_delay_s = QQBOT_LONG_CONN_RETRY_BASE_S;
                        self.record_runtime_warn(
                            qqbot::QQBOT_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_intents_downgraded",
                            format!(
                                "qqbot long connection downgrade intents: account_id={}, from={}, to={}, reason={}",
                                target.account_id, previous_intents, next_intents, err_text
                            ),
                        );
                        debug!(
                            "qqbot long connection downgrade intents: account_id={}, from={}, to={}, reason={}",
                            target.account_id, previous_intents, next_intents, err_text
                        );
                    } else {
                        self.record_runtime_warn(
                            qqbot::QQBOT_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_failed",
                            format!(
                                "qqbot long connection failed: account_id={}, retry_in={}s, intents={}, error={}",
                                target.account_id, retry_delay_s, intents, err_text
                            ),
                        );
                        debug!(
                            "qqbot long connection failed: account_id={}, retry_in={}s, intents={}, error={}",
                            target.account_id, retry_delay_s, intents, err_text
                        );
                        retry_delay_s = (retry_delay_s * 2).min(QQBOT_LONG_CONN_RETRY_MAX_S);
                    }
                }
            }

            sleep(Duration::from_secs(retry_delay_s)).await;
        }
    }

    async fn handle_qqbot_long_connection_payload(
        &self,
        target: &QqBotLongConnTarget,
        payload: Value,
    ) -> Result<()> {
        let event_type = qqbot::dispatch_event_type(&payload).unwrap_or_default();
        let messages = match qqbot::extract_dispatch_messages(&payload, &target.account_id) {
            Ok(value) => value,
            Err(err) => {
                self.record_runtime_warn(
                    qqbot::QQBOT_CHANNEL,
                    Some(&target.account_id),
                    "long_connection_dispatch_parse_failed",
                    format!("qqbot long connection dispatch parse failed: event_type={event_type}, error={err}"),
                );
                return Ok(());
            }
        };
        if messages.is_empty() {
            return Ok(());
        }
        let headers = build_internal_channel_headers(target.inbound_token.as_deref())?;
        let result = match self
            .enqueue_inbound(
                qqbot::QQBOT_CHANNEL,
                &headers,
                messages,
                Some(payload.clone()),
            )
            .await
        {
            Ok(value) => value,
            Err(err) => {
                self.record_runtime_error(
                    qqbot::QQBOT_CHANNEL,
                    Some(&target.account_id),
                    "long_connection_inbound_enqueue_failed",
                    format!("qqbot long connection inbound enqueue failed: event_type={event_type}, error={err}"),
                );
                return Ok(());
            }
        };
        if result.accepted == 0 {
            self.record_runtime_warn(
                qqbot::QQBOT_CHANNEL,
                Some(&target.account_id),
                "long_connection_inbound_ignored",
                format!(
                    "qqbot long connection inbound ignored: event_type={event_type}, accepted=0"
                ),
            );
            return Ok(());
        }

        self.record_runtime_info(
            qqbot::QQBOT_CHANNEL,
            Some(&target.account_id),
            "long_connection_inbound_received",
            format!(
                "qqbot long connection inbound accepted: event_type={event_type}, accepted={}",
                result.accepted
            ),
        );
        Ok(())
    }

    pub(super) async fn xmpp_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(XMPP_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_xmpp_long_connection_targets().await {
                Ok(targets) => {
                    let mut desired_keys = HashSet::new();
                    for target in targets {
                        let task_key = target.task_key();
                        desired_keys.insert(task_key.clone());
                        if workers.contains_key(&task_key) {
                            continue;
                        }
                        let worker = self.clone();
                        workers.insert(
                            task_key,
                            long_task::spawn("channels.long_connection.xmpp.worker", async move {
                                worker.xmpp_long_connection_worker_loop(target).await;
                            }),
                        );
                    }

                    let stale_keys = workers
                        .keys()
                        .filter(|key| !desired_keys.contains(*key))
                        .cloned()
                        .collect::<Vec<_>>();
                    for task_key in stale_keys {
                        if let Some(handle) = workers.remove(&task_key) {
                            handle.abort();
                        }
                    }
                }
                Err(err) => {
                    self.record_runtime_warn(
                        xmpp::XMPP_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load xmpp long connection targets failed: {err}"),
                    );
                    debug!("load xmpp long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(XMPP_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_xmpp_long_connection_targets(&self) -> Result<Vec<XmppLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts = blocking::run_db("channels.long_connection.xmpp.list_accounts", move || {
            storage.list_channel_accounts(Some(xmpp::XMPP_CHANNEL), Some("active"))
        })
        .await?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(xmpp_cfg) = account_cfg.xmpp else {
                continue;
            };
            if !xmpp::long_connection_enabled(&xmpp_cfg) {
                continue;
            }
            if !xmpp::has_long_connection_credentials(&xmpp_cfg) {
                self.record_runtime_warn(
                    xmpp::XMPP_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip xmpp long connection target without credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
                    "skip xmpp long connection target without credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(XmppLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: xmpp_cfg,
            });
        }
        Ok(targets)
    }

    async fn xmpp_long_connection_worker_loop(&self, target: XmppLongConnTarget) {
        let mut retry_delay_s = XMPP_LONG_CONN_RETRY_BASE_S;
        loop {
            let result = xmpp::run_long_connection_session(&target.account_id, &target.config, {
                let worker = self.clone();
                let event_target = target.clone();
                move |message| {
                    let worker = worker.clone();
                    let event_target = event_target.clone();
                    async move {
                        worker
                            .handle_xmpp_long_connection_message(&event_target, message)
                            .await
                    }
                }
            })
            .await;

            match result {
                Ok(()) => {
                    retry_delay_s = XMPP_LONG_CONN_RETRY_BASE_S;
                    self.record_runtime_warn(
                        xmpp::XMPP_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_closed",
                        format!(
                            "xmpp long connection closed: account_id={}, retry_in={}s",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "xmpp long connection closed: account_id={}, retry_in={}s",
                        target.account_id, retry_delay_s
                    );
                }
                Err(err) => {
                    self.record_runtime_warn(
                        xmpp::XMPP_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_failed",
                        format!(
                            "xmpp long connection failed: account_id={}, retry_in={}s, error={err}",
                            target.account_id, retry_delay_s
                        ),
                    );
                    debug!(
                        "xmpp long connection failed: account_id={}, retry_in={}s, error={err}",
                        target.account_id, retry_delay_s
                    );
                    retry_delay_s = (retry_delay_s * 2).min(XMPP_LONG_CONN_RETRY_MAX_S);
                }
            }

            sleep(Duration::from_secs(retry_delay_s)).await;
        }
    }

    async fn handle_xmpp_long_connection_message(
        &self,
        target: &XmppLongConnTarget,
        message: ChannelMessage,
    ) -> Result<()> {
        let headers = build_internal_channel_headers(target.inbound_token.as_deref())?;
        let result = self
            .enqueue_inbound(xmpp::XMPP_CHANNEL, &headers, vec![message], None)
            .await?;
        if result.accepted == 0 {
            return Err(anyhow!(
                "xmpp long connection inbound ignored: no message accepted"
            ));
        }
        Ok(())
    }

    pub(super) async fn weixin_long_connection_supervisor_loop(&self) {
        let mut workers: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
        loop {
            workers.retain(|_, handle| !handle.is_finished());
            let config = self.config_store.get().await;
            if !channels_runtime_enabled(&config) {
                for (_, handle) in workers.drain() {
                    handle.abort();
                }
                sleep(Duration::from_secs(WEIXIN_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
                continue;
            }

            match self.list_weixin_long_connection_targets().await {
                Ok(targets) => {
                    let mut desired_keys = HashSet::new();
                    for target in targets {
                        let task_key = target.task_key();
                        desired_keys.insert(task_key.clone());
                        if workers.contains_key(&task_key) {
                            continue;
                        }
                        let worker = self.clone();
                        workers.insert(
                            task_key,
                            long_task::spawn(
                                "channels.long_connection.weixin.worker",
                                async move {
                                    worker.weixin_long_connection_worker_loop(target).await;
                                },
                            ),
                        );
                    }

                    let stale_keys = workers
                        .keys()
                        .filter(|key| !desired_keys.contains(*key))
                        .cloned()
                        .collect::<Vec<_>>();
                    for task_key in stale_keys {
                        if let Some(handle) = workers.remove(&task_key) {
                            handle.abort();
                        }
                    }
                }
                Err(err) => {
                    self.record_runtime_warn(
                        weixin::WEIXIN_CHANNEL,
                        None,
                        "long_connection_targets_load_failed",
                        format!("load weixin long connection targets failed: {err}"),
                    );
                    debug!("load weixin long connection targets failed: {err}");
                }
            }

            sleep(Duration::from_secs(WEIXIN_LONG_CONN_SUPERVISOR_INTERVAL_S)).await;
        }
    }

    async fn list_weixin_long_connection_targets(&self) -> Result<Vec<WeixinLongConnTarget>> {
        let storage = self.storage.clone();
        let accounts =
            blocking::run_db("channels.long_connection.weixin.list_accounts", move || {
                storage.list_channel_accounts(Some(weixin::WEIXIN_CHANNEL), Some("active"))
            })
            .await?;

        let mut targets = Vec::new();
        for record in accounts {
            let account_cfg = ChannelAccountConfig::from_value(&record.config);
            let Some(weixin_cfg) = account_cfg.weixin else {
                continue;
            };
            if !weixin::long_connection_enabled(&weixin_cfg) {
                continue;
            }
            if !weixin::has_long_connection_credentials(&weixin_cfg) {
                self.record_runtime_warn(
                    weixin::WEIXIN_CHANNEL,
                    Some(&record.account_id),
                    "long_connection_credentials_missing",
                    format!(
                        "skip weixin long connection target without credentials: account_id={}",
                        record.account_id
                    ),
                );
                debug!(
                    "skip weixin long connection target without credentials: account_id={}",
                    record.account_id
                );
                continue;
            }
            targets.push(WeixinLongConnTarget {
                account_id: record.account_id,
                updated_at: record.updated_at,
                inbound_token: account_cfg.inbound_token,
                config: weixin_cfg,
            });
        }
        Ok(targets)
    }

    async fn weixin_long_connection_worker_loop(&self, target: WeixinLongConnTarget) {
        let max_failures = weixin::resolve_max_consecutive_failures(&target.config);
        let backoff_ms = weixin::resolve_backoff_ms(&target.config);
        let mut next_poll_timeout_ms = weixin::resolve_poll_timeout_ms(&target.config);
        let mut get_updates_buf = String::new();
        let mut consecutive_failures = 0_u64;

        loop {
            let response = weixin::get_updates(
                &self.http,
                &target.config,
                &get_updates_buf,
                next_poll_timeout_ms,
            )
            .await;

            match response {
                Ok(result) => {
                    if let Some(timeout_ms) =
                        result.longpolling_timeout_ms.filter(|value| *value > 0)
                    {
                        next_poll_timeout_ms = timeout_ms.max(1_000);
                    }
                    if let Some(buf) = result
                        .get_updates_buf
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        get_updates_buf = buf.to_string();
                    }

                    let errcode = result.errcode.unwrap_or(0);
                    let has_error = result.ret != 0 || errcode != 0;
                    if has_error {
                        let errmsg = result.errmsg.as_deref().unwrap_or("unknown");
                        self.record_runtime_warn(
                            weixin::WEIXIN_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_poll_failed",
                            format!(
                                "weixin getupdates failed: account_id={}, ret={}, errcode={}, errmsg={}",
                                target.account_id, result.ret, errcode, errmsg
                            ),
                        );
                        if errcode == -14 || result.ret == -14 {
                            self.record_runtime_error(
                                weixin::WEIXIN_CHANNEL,
                                Some(&target.account_id),
                                "long_connection_session_expired",
                                format!(
                                    "weixin login session expired: account_id={}, retry_in={}ms",
                                    target.account_id, backoff_ms
                                ),
                            );
                            sleep(Duration::from_millis(backoff_ms)).await;
                            continue;
                        }
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        if consecutive_failures >= max_failures {
                            self.record_runtime_warn(
                                weixin::WEIXIN_CHANNEL,
                                Some(&target.account_id),
                                "long_connection_backoff",
                                format!(
                                    "weixin getupdates reached failure threshold: account_id={}, failures={}, backoff_ms={}",
                                    target.account_id, consecutive_failures, backoff_ms
                                ),
                            );
                            consecutive_failures = 0;
                            sleep(Duration::from_millis(backoff_ms)).await;
                        } else {
                            sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                        }
                        continue;
                    }

                    consecutive_failures = 0;
                    if result.msgs.is_empty() {
                        continue;
                    }

                    let messages = weixin::extract_inbound_messages(
                        &result.msgs,
                        &target.account_id,
                        &target.config,
                    );
                    if messages.is_empty() {
                        continue;
                    }

                    let headers = match build_internal_channel_headers(
                        target.inbound_token.as_deref(),
                    ) {
                        Ok(value) => value,
                        Err(err) => {
                            self.record_runtime_error(
                                    weixin::WEIXIN_CHANNEL,
                                    Some(&target.account_id),
                                    "long_connection_header_build_failed",
                                    format!(
                                        "weixin long connection build headers failed: account_id={}, error={err}",
                                        target.account_id
                                    ),
                                );
                            sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                            continue;
                        }
                    };

                    let raw_payload = json!({
                        "ret": result.ret,
                        "errcode": result.errcode,
                        "errmsg": result.errmsg,
                        "msgs_count": result.msgs.len(),
                        "get_updates_buf": result.get_updates_buf,
                    });
                    match self
                        .enqueue_inbound(
                            weixin::WEIXIN_CHANNEL,
                            &headers,
                            messages,
                            Some(raw_payload),
                        )
                        .await
                    {
                        Ok(outcome) => {
                            if outcome.accepted == 0 {
                                self.record_runtime_warn(
                                    weixin::WEIXIN_CHANNEL,
                                    Some(&target.account_id),
                                    "long_connection_inbound_ignored",
                                    format!(
                                        "weixin long connection inbound ignored: account_id={}, accepted=0",
                                        target.account_id
                                    ),
                                );
                            } else {
                                self.record_runtime_info(
                                    weixin::WEIXIN_CHANNEL,
                                    Some(&target.account_id),
                                    "long_connection_inbound_received",
                                    format!(
                                        "weixin long connection inbound accepted: account_id={}, accepted={}",
                                        target.account_id, outcome.accepted
                                    ),
                                );
                            }
                        }
                        Err(err) => {
                            self.record_runtime_error(
                                weixin::WEIXIN_CHANNEL,
                                Some(&target.account_id),
                                "long_connection_inbound_enqueue_failed",
                                format!(
                                    "weixin long connection inbound enqueue failed: account_id={}, error={err}",
                                    target.account_id
                                ),
                            );
                            sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                        }
                    }
                }
                Err(err) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    self.record_runtime_warn(
                        weixin::WEIXIN_CHANNEL,
                        Some(&target.account_id),
                        "long_connection_poll_error",
                        format!(
                            "weixin getupdates error: account_id={}, failures={}/{}, error={err}",
                            target.account_id, consecutive_failures, max_failures
                        ),
                    );
                    if consecutive_failures >= max_failures {
                        self.record_runtime_warn(
                            weixin::WEIXIN_CHANNEL,
                            Some(&target.account_id),
                            "long_connection_backoff",
                            format!(
                                "weixin getupdates entered backoff: account_id={}, failures={}, backoff_ms={}",
                                target.account_id, consecutive_failures, backoff_ms
                            ),
                        );
                        consecutive_failures = 0;
                        sleep(Duration::from_millis(backoff_ms)).await;
                    } else {
                        sleep(Duration::from_millis(WEIXIN_LONG_CONN_RETRY_BASE_MS)).await;
                    }
                }
            }
        }
    }
}
