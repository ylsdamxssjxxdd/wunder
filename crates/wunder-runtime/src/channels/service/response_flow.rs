use super::support::{
    append_weixin_context_token_from_message, build_bridge_session_metadata,
    merge_object_value_into, message_preview_text, truncate_text,
};
use super::{
    ChannelCommand, ChannelHub, ChannelInboundResult, ChannelSessionInfo, ChannelSessionStrategy,
};
use crate::channels::binding::BindingResolution;
use crate::channels::types::{
    ChannelMessage, ChannelOutboundMessage, FeishuConfig, WeixinConfig, XmppConfig,
};
use crate::channels::{feishu, weixin, xmpp};
use crate::core::approval::ApprovalResponse;
use crate::services::bridge::{append_bridge_meta, BridgeRouteResolution};
use anyhow::Result;
use serde_json::{json, Value};
use tracing::warn;
use uuid::Uuid;

impl ChannelHub {
    pub(super) async fn respond_busy(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        append_user_turn: bool,
        agent_display_name: Option<&str>,
        processing_ack_message_id: Option<&str>,
    ) -> Result<ChannelInboundResult> {
        let last_message = self
            .load_latest_user_message(&session_info.user_id, &session_info.session_id)
            .await
            .unwrap_or_default();
        let agent_display_name = agent_display_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("智能体");
        let busy_text = if last_message.trim().is_empty() {
            format!("正在忙，请稍后再试（{agent_display_name}）。")
        } else {
            let preview = truncate_text(last_message.trim(), 120);
            format!("正在忙：{preview}（{agent_display_name}）。")
        };
        let user_text = message_preview_text(message);
        if append_user_turn {
            self.append_channel_chat(
                &session_info.user_id,
                &session_info.session_id,
                "user",
                &user_text,
            )
            .await;
        } else {
            self.append_channel_chat_history(
                &session_info.user_id,
                &session_info.session_id,
                "user",
                &user_text,
                None,
            )
            .await;
        }
        self.append_channel_chat(
            &session_info.user_id,
            &session_info.session_id,
            "assistant",
            &busy_text,
        )
        .await;
        let mut outbound_meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "busy": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut outbound_meta, bridge_meta);
        }
        if let Some(ack_message_id) = processing_ack_message_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(meta_obj) = outbound_meta.as_object_mut() {
                meta_obj.insert(
                    "processing_ack_message_id".to_string(),
                    Value::String(ack_message_id.to_string()),
                );
            }
        }
        append_weixin_context_token_from_message(&mut outbound_meta, message);
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(busy_text.clone()),
            attachments: Vec::new(),
            meta: Some(outbound_meta),
        };
        let outbox_id = self.enqueue_outbox(&outbound).await?;
        Ok(ChannelInboundResult {
            session_id: session_info.session_id.clone(),
            outbox_id: Some(outbox_id),
        })
    }

    pub(super) async fn send_processing_ack(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        feishu_cfg: &FeishuConfig,
    ) -> Result<Option<String>> {
        let ack_text = "已收到消息，正在处理中，请稍后。".to_string();
        let mut meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "processing_ack": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut meta, bridge_meta);
        }
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(ack_text),
            attachments: Vec::new(),
            meta: Some(meta),
        };
        let result = feishu::send_outbound(&self.http, &outbound, feishu_cfg).await?;
        Ok(result.message_id)
    }

    pub(super) async fn send_weixin_processing_ack(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        weixin_cfg: &WeixinConfig,
        agent_display_name: &str,
    ) -> Result<()> {
        let cleaned_agent_name = agent_display_name.trim();
        let ack_text = if cleaned_agent_name.is_empty() {
            "已收到消息，正在处理中，请稍后。".to_string()
        } else {
            format!("已收到消息，{cleaned_agent_name}正在处理中，请稍后。")
        };
        let mut meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "processing_ack": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut meta, bridge_meta);
        }
        append_weixin_context_token_from_message(&mut meta, message);
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(ack_text),
            attachments: Vec::new(),
            meta: Some(meta),
        };
        weixin::send_outbound(&self.http, &outbound, weixin_cfg).await?;
        Ok(())
    }

    pub(super) async fn send_xmpp_processing_ack(
        &self,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        resolved_binding: Option<&BindingResolution>,
        xmpp_cfg: &XmppConfig,
    ) -> Result<()> {
        let ack_text = "已收到消息，正在处理中，请稍后。".to_string();
        let mut meta = json!({
            "session_id": session_info.session_id,
            "binding_id": resolved_binding.and_then(|b| b.binding_id.clone()),
            "message_id": message.message_id,
            "processing_ack": true,
        });
        if let Some(bridge_meta) = self.load_channel_session_bridge_metadata(message).await? {
            merge_object_value_into(&mut meta, bridge_meta);
        }
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(ack_text),
            attachments: Vec::new(),
            meta: Some(meta),
        };
        xmpp::send_outbound(&message.account_id, &outbound, xmpp_cfg).await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn handle_channel_command(
        &self,
        command: ChannelCommand,
        message: &ChannelMessage,
        session_info: &ChannelSessionInfo,
        bridge_resolution: Option<&BridgeRouteResolution>,
        agent_id: Option<&str>,
        tool_names: &[String],
        tts_enabled: Option<bool>,
        tts_voice: Option<&str>,
        session_strategy: ChannelSessionStrategy,
    ) -> Result<ChannelInboundResult> {
        let user_id = session_info.user_id.clone();
        let command_text = message.text.as_deref().map(str::trim).unwrap_or("");
        let (target_session_id, reply_text) = match command {
            ChannelCommand::NewThread => {
                let new_session_id = format!("sess_{}", Uuid::new_v4().simple());
                let agent_key = agent_id.unwrap_or("").trim();
                if let Err(err) = self
                    .thread_runtime
                    .set_main_session(&user_id, agent_key, &new_session_id, "channel_command")
                    .await
                {
                    warn!(
                        "channel /new failed to set main session: user_id={}, agent_id={}, error={err}",
                        user_id, agent_key
                    );
                }
                let updated = self
                    .resolve_channel_session(
                        message,
                        agent_id,
                        tool_names,
                        tts_enabled,
                        tts_voice,
                        session_strategy,
                        Some(user_id.clone()),
                        bridge_resolution.map(build_bridge_session_metadata),
                    )
                    .await?;
                (updated.session_id, "已创建新线程。".to_string())
            }
            ChannelCommand::Stop => {
                self.clear_pending_channel_approvals(
                    Some(&session_info.session_id),
                    None,
                    ApprovalResponse::Deny,
                )
                .await;
                let cancelled = self.monitor.cancel(&session_info.session_id);
                (
                    session_info.session_id.clone(),
                    if cancelled {
                        "已请求停止当前会话。".to_string()
                    } else {
                        "当前没有可停止的会话。".to_string()
                    },
                )
            }
            ChannelCommand::Help => (
                session_info.session_id.clone(),
                "可用指令：/new 新建线程，/stop 停止当前会话。若收到审批提示，请回复 1/2/3。"
                    .to_string(),
            ),
        };

        if !command_text.is_empty() {
            self.append_channel_chat(&user_id, &target_session_id, "user", command_text)
                .await;
        }
        self.append_channel_chat(&user_id, &target_session_id, "assistant", &reply_text)
            .await;

        let mut outbound_meta = json!({
            "session_id": target_session_id,
            "command": command.as_str(),
            "message_id": message.message_id,
        });
        if let Some(resolution) = bridge_resolution {
            append_bridge_meta(&mut outbound_meta, resolution);
        }
        append_weixin_context_token_from_message(&mut outbound_meta, message);
        let outbound = ChannelOutboundMessage {
            channel: message.channel.clone(),
            account_id: message.account_id.clone(),
            peer: message.peer.clone(),
            thread: message.thread.clone(),
            text: Some(reply_text.clone()),
            attachments: Vec::new(),
            meta: Some(outbound_meta),
        };
        let outbox_id = self.enqueue_outbox(&outbound).await?;
        Ok(ChannelInboundResult {
            session_id: session_info.session_id.clone(),
            outbox_id: Some(outbox_id),
        })
    }
}
