use super::*;
use crate::services::runtime::thread::mailbox::AgentMessage;

pub(super) fn message(context: &ToolContext<'_>, args: &Value, kind: &str) -> Result<AgentMessage> {
    let text = args
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let id = args
        .get("message_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple()));
    let message = AgentMessage {
        id,
        source: context.session_id.to_string(),
        kind: kind.to_string(),
        text,
        cancellation: context
            .monitor
            .as_ref()
            .and_then(|monitor| monitor.child_run_token(context.session_id)),
    };
    message.validate()?;
    if context
        .monitor
        .as_ref()
        .is_some_and(|monitor| monitor.is_cancelled(context.session_id))
    {
        return Err(anyhow!("message source was interrupted"));
    }
    Ok(message)
}

pub(super) fn steer(
    context: &ToolContext<'_>,
    target: &str,
    message: &AgentMessage,
) -> Result<bool> {
    let monitor = context
        .monitor
        .as_ref()
        .ok_or_else(|| anyhow!("monitor unavailable"))?;
    if !monitor.mailboxes.is_open(context.user_id, target) {
        return Ok(false);
    }
    if monitor.is_cancelled(target) {
        return Err(anyhow!("target is interrupted; wait for settlement"));
    }
    let accepted = monitor
        .mailboxes
        .send(context.user_id, target, message.clone())?;
    if accepted {
        monitor.run_signals.notify(target);
    }
    Ok(accepted)
}

pub(super) fn receipt(action: &str, target: &str, message: &AgentMessage, delivery: &str) -> Value {
    build_model_tool_success(
        action,
        "accepted",
        "Message accepted; application is reported separately.",
        json!({"session_id":target,"message_id":message.id,"delivery":delivery}),
    )
}

pub(super) async fn report(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let message = message(context, args, "report")?;
    let store = context.storage.clone();
    let user = context.user_id.to_string();
    let source = context.session_id.to_string();
    let parent = crate::core::blocking::run_db("subagent.report.parent", move || {
        let child = store
            .get_chat_session(&user, &source)?
            .ok_or_else(|| anyhow!("child session not found"))?;
        if !matches!(
            child.spawned_by.as_deref(),
            Some("model" | "subagent_control")
        ) {
            return Err(anyhow!(
                "report is only available to a temporary child agent"
            ));
        }
        let parent = child
            .parent_session_id
            .ok_or_else(|| anyhow!("parent session missing"))?;
        let parent = store
            .get_chat_session(&user, &parent)?
            .ok_or_else(|| anyhow!("parent session not found"))?;
        if parent.status != "active" {
            return Err(anyhow!("parent session is closed"));
        }
        Ok(parent)
    })
    .await?;
    let orchestrator = context
        .orchestrator
        .as_ref()
        .ok_or_else(|| anyhow!("orchestrator unavailable"))?;
    let runtime = orchestrator
        .task_runtime
        .read()
        .upgrade()
        .ok_or_else(|| anyhow!("thread runtime unavailable"))?;
    let request = crate::services::subagents::build_parent_auto_wake_request(
        context.storage.as_ref(),
        context.user_id,
        &parent.session_id,
        context.request_config_overrides,
        json!({"type":"subagent_message","kind":"report","message_id":message.id,
            "source_session_id":context.session_id,"message":message.text}),
    )?;
    // A retry of an idle-parent delivery must not also enter a newly opened inbox.
    let existing = if args.get("message_id").is_some() {
        runtime.existing_agent_message(&request, &message).await?
    } else {
        None
    };
    let queue_id = if let Some(id) = existing {
        id
    } else {
        if steer(context, &parent.session_id, &message)? {
            return Ok(receipt(
                "report",
                &parent.session_id,
                &message,
                "queued_current_turn",
            ));
        }
        runtime.submit_agent_message(request, &message).await?
    };
    let mut result = receipt("report", &parent.session_id, &message, "queued_next_turn");
    result["data"]["queue_id"] = json!(queue_id);
    Ok(result)
}
