//! Typed cloud thread façade. All calls use the engine cloud client and ledger.
use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use wunder_server::{cloud::CloudThreadWatch, stable_core::thread_view::ThreadViewPage};

#[derive(Debug, Clone)]
pub struct NativeCloudAgent {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Clone)]
pub struct NativeCloudThread {
    pub id: String,
    pub title: String,
    pub status: String,
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
    pub fn cloud_thread_view(&self, thread: &str) -> Result<ThreadViewPage> {
        let data = self
            .runtime
            .block_on(self.state().cloud.cloud_thread_view(thread))?;
        Ok(serde_json::from_value(data["page"].clone())?)
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
