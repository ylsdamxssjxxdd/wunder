//! Typed cloud thread façade. All calls use the engine cloud client and ledger.
use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use wunder_server::{
    cloud::CloudThreadWatch,
    stable_core::thread_view::{BodyRef, ThreadBodyPage, ThreadViewPage, ViewMessage},
};

#[derive(Debug, Clone)]
pub struct NativeCloudAgent {
    pub id: String,
    pub name: String,
    pub description: String,
}
#[derive(Debug, Clone)]
pub struct NativeCloudThread {
    pub id: String,
    pub title: String,
    pub status: String,
}

pub struct NativeCloudThreadView {
    pub page: ThreadViewPage,
    pub model_key: String,
    pub reasoning_effort: String,
    pub agent_id: String,
}

impl NativeDesktop {
    pub fn cloud_agents(&self) -> Result<Vec<NativeCloudAgent>> {
        let data = self.runtime.block_on(self.state().cloud.cloud_agents())?;
        Ok(data["items"]
            .as_array()
            .into_iter()
            .flatten()
            .take(50)
            .map(|row| NativeCloudAgent {
                id: string(row, "id"),
                name: string(row, "name"),
                description: string(row, "description"),
            })
            .collect())
    }
    pub fn cloud_threads(&self, offset: usize) -> Result<Vec<NativeCloudThread>> {
        let data = self.cloud_thread_command(
            "threads.list",
            json!({"offset":offset.min(1_000_000),"limit":50}),
        )?;
        Ok(data["threads"]
            .as_array()
            .into_iter()
            .flatten()
            .take(50)
            .map(|row| NativeCloudThread {
                id: string(row, "local_thread_id"),
                title: string(row, "title"),
                status: string(row, "status"),
            })
            .collect())
    }
    /// Create an empty cloud thread before the first message.  Keeping this as
    /// an explicit operation makes the workspace tree immediately consistent
    /// with local workspaces and avoids turning a retry into a duplicate turn.
    pub fn cloud_thread_create(&self, agent: &str, title: &str) -> Result<NativeCloudThread> {
        let mut args = json!({});
        if !agent.trim().is_empty() {
            args["agent"] = json!(agent.trim());
        }
        if !title.trim().is_empty() {
            args["title"] = json!(title.trim());
        }
        let result = self.cloud_thread_command("thread.create", args)?;
        let id = string(&result, "local_thread_id");
        anyhow::ensure!(!id.is_empty(), "cloud command returned no thread id");
        Ok(NativeCloudThread {
            id,
            title: string(&result, "title"),
            status: "active".to_string(),
        })
    }
    pub fn cloud_thread_view(&self, thread: &str) -> Result<ThreadViewPage> {
        Ok(self.cloud_thread_bootstrap(thread)?.page)
    }

    pub fn cloud_thread_bootstrap(&self, thread: &str) -> Result<NativeCloudThreadView> {
        let data = self
            .runtime
            .block_on(self.state().cloud.cloud_thread_view(thread))?;
        Ok(NativeCloudThreadView {
            page: serde_json::from_value(data["page"].clone())?,
            model_key: string(&data["session"], "model_key"),
            reasoning_effort: string(&data["session"], "reasoning_effort"),
            agent_id: string(&data["session"], "agent_id"),
        })
    }

    pub fn cloud_thread_models(&self) -> Result<Vec<String>> {
        let data = self
            .runtime
            .block_on(self.state().cloud.cloud_thread_models())?;
        Ok(data["items"]
            .as_array()
            .into_iter()
            .flatten()
            .take(100)
            .filter_map(|row| row["name"].as_str().or_else(|| row["key"].as_str()))
            .map(str::to_string)
            .collect())
    }

    pub fn cloud_thread_system_prompt(&self, thread: &str) -> Result<String> {
        self.runtime
            .block_on(self.state().cloud.cloud_thread_system_prompt(thread))
    }

    pub fn cloud_thread_set_model(&self, thread: &str, model: &str) -> Result<()> {
        self.runtime
            .block_on(self.state().cloud.cloud_thread_set_model(thread, model))
    }

    pub fn cloud_thread_set_effort(&self, thread: &str, effort: &str) -> Result<String> {
        self.runtime
            .block_on(self.state().cloud.cloud_thread_set_effort(thread, effort))
    }
    pub fn cloud_thread_history(&self, thread: &str, before: i64) -> Result<ThreadViewPage> {
        let data = self
            .runtime
            .block_on(self.state().cloud.cloud_thread_chat_page(thread, before))?;
        Ok(serde_json::from_value(data["page"].clone())?)
    }

    pub fn cloud_thread_rename(&self, thread: &str, title: &str) -> Result<()> {
        self.runtime
            .block_on(self.state().cloud.cloud_thread_rename(thread, title))
    }

    pub fn cloud_thread_archive(&self, thread: &str) -> Result<()> {
        self.runtime
            .block_on(self.state().cloud.cloud_thread_archive(thread))
    }

    pub fn cloud_thread_trajectory_page(
        &self,
        thread: &str,
        before: Option<i64>,
        after: Option<i64>,
        limit: usize,
    ) -> Result<Value> {
        let data = self.runtime.block_on(
            self.state()
                .cloud
                .cloud_thread_trajectory(thread, before, after, limit),
        )?;
        Ok(data["page"].clone())
    }

    pub fn cloud_thread_trajectory_detail(
        &self,
        thread: &str,
        body: &str,
        revision: i64,
    ) -> Result<Value> {
        let text = self.cloud_body_text(
            thread,
            &BodyRef {
                body_id: body.into(),
                revision,
                bytes: 0,
            },
        )?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn cloud_thread_search(&self, thread: &str, query: &str) -> Result<Value> {
        self.runtime
            .block_on(self.state().cloud.cloud_thread_search(thread, query))
    }

    pub fn cloud_thread_body(
        &self,
        thread: &str,
        body: &str,
        revision: i64,
        cursor: i64,
    ) -> Result<Value> {
        self.runtime.block_on(
            self.state()
                .cloud
                .cloud_thread_body(thread, body, revision, cursor),
        )
    }

    pub fn cloud_thread_process_page(
        &self,
        thread: &str,
        root: &str,
        final_id: &str,
        before: Option<i64>,
        after: Option<i64>,
    ) -> Result<super::NativeProcessPage> {
        let page = self.runtime.block_on(
            self.state()
                .cloud
                .cloud_thread_process(thread, root, before, after),
        )?;
        Ok(super::chat_process::project_page(&page["turn"], final_id))
    }

    pub fn cloud_export_thread_log(&self, thread: &str, path: &std::path::Path) -> Result<()> {
        self.runtime
            .block_on(self.state().cloud.cloud_export_thread_log(thread, path))
    }

    /// Copy complete content on demand. Preview/tail text is never substituted
    /// for an incomplete body; oversized clipboard payloads fail explicitly.
    pub fn cloud_message_text(&self, thread: &str, message: &ViewMessage) -> Result<String> {
        if let Some(body) = &message.body {
            self.cloud_body_text(thread, body)
        } else {
            anyhow::ensure!(
                message.tail_offset == 0
                    && message.content_offset <= message.content.encode_utf16().count(),
                "full message body is unavailable; refresh the thread"
            );
            Ok(if message.tail.is_empty() {
                message.content.clone()
            } else {
                message.tail.clone()
            })
        }
    }

    fn cloud_body_text(&self, thread: &str, body: &BodyRef) -> Result<String> {
        let cloud = self.state().cloud.clone();
        let identity = cloud.session().ok_or_else(|| anyhow!("not logged in"))?;
        self.runtime.block_on(async {
            let mut cursor = 0;
            let mut text = String::new();
            for _ in 0..1024 {
                let live = cloud
                    .session()
                    .ok_or_else(|| anyhow!("cloud session ended"))?;
                anyhow::ensure!(
                    live.server == identity.server
                        && live.user_id == identity.user_id
                        && live.device_id == identity.device_id
                        && live.logged_in_at == identity.logged_in_at,
                    "cloud account changed; refresh the thread"
                );
                let page: ThreadBodyPage = serde_json::from_value(
                    cloud
                        .cloud_thread_body(thread, &body.body_id, body.revision, cursor)
                        .await?,
                )?;
                anyhow::ensure!(
                    page.body_id == body.body_id && page.revision == body.revision,
                    "cloud body revision changed; refresh the thread"
                );
                for chunk in page.chunks {
                    anyhow::ensure!(
                        text.len() + chunk.len() <= 8 * 1024 * 1024,
                        "message exceeds the clipboard limit (8 MiB); use a smaller body selection"
                    );
                    text.push_str(&chunk);
                }
                if !page.has_more {
                    anyhow::ensure!(
                        body.bytes == 0 || text.len() == body.bytes,
                        "message body is incomplete; refresh the thread"
                    );
                    return Ok(text);
                }
                let next = page
                    .next_cursor
                    .ok_or_else(|| anyhow!("missing body cursor"))?;
                anyhow::ensure!(next > cursor, "body cursor did not advance");
                cursor = next;
            }
            Err(anyhow!("message body exceeds the page limit"))
        })
    }
    pub fn cloud_thread_watch(&self, thread: &str, after: i64) -> CloudThreadWatch {
        let _guard = self.runtime.enter();
        self.state()
            .cloud
            .watch_cloud_thread(thread.to_string(), after)
    }
    pub fn cloud_thread_send(&self, thread: &str, agent: &str, message: &str) -> Result<String> {
        anyhow::ensure!(
            !message.trim().is_empty() && message.len() <= 128 * 1024,
            "message is empty or too large"
        );
        let mut args = json!({"message":message,"agent":agent});
        if !thread.trim().is_empty() {
            args["local_thread_id"] = json!(thread);
        }
        let result = self.cloud_thread_command("thread.message", args)?;
        let id = string(&result, "local_thread_id");
        anyhow::ensure!(!id.is_empty(), "cloud command returned no thread id");
        Ok(id)
    }
    pub fn cloud_thread_cancel(&self, thread: &str) -> Result<()> {
        self.cloud_thread_command("thread.cancel", json!({"local_thread_id":thread}))?;
        Ok(())
    }
    pub fn cloud_thread_answer(&self, thread: &str, approval: &str, approve: bool) -> Result<()> {
        self.cloud_thread_command("thread.answer", json!({"local_thread_id":thread,"approval_id":approval,"decision":if approve {"approve_once"} else {"deny"}}))?;
        Ok(())
    }
    fn cloud_thread_command(&self, kind: &str, args: Value) -> Result<Value> {
        let cloud = self.state().cloud.clone();
        self.runtime.block_on(async {
            let issue = cloud.interlink_issue_command("cloud", kind, args).await?;
            let id = issue["command_id"]
                .as_str()
                .ok_or_else(|| anyhow!("missing command id"))?;
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let record = cloud.interlink_command(id).await?;
                match record["status"].as_str().unwrap_or("") {
                    "succeeded" => {
                        anyhow::ensure!(
                            !record["result"].is_null(),
                            "RESULT_EXPIRED: cloud result expired; refresh the view"
                        );
                        return Ok(record["result"].clone());
                    }
                    "failed" | "canceled" | "timeout" => {
                        return Err(anyhow!(
                            "{}: {}",
                            string(&record, "error_code"),
                            string(&record, "error_summary")
                        ))
                    }
                    _ => {}
                }
                if Instant::now() >= deadline {
                    let _ = cloud.interlink_cancel_command(id).await;
                    return Err(anyhow!(
                        "cloud command timed out; cancellation requested ({id})"
                    ));
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
    }
}
fn string(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or("").to_string()
}
