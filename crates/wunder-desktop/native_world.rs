use super::NativeDesktop;
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct WorldContact {
    pub user_id: String,
    pub username: String,
    pub online: bool,
    pub conversation_id: String,
    pub preview: String,
    pub unread: i64,
}

#[derive(Clone, Debug)]
pub struct WorldGroup {
    pub group_id: String,
    pub conversation_id: String,
    pub name: String,
    pub members: i64,
    pub preview: String,
    pub unread: i64,
}

#[derive(Clone, Debug)]
pub struct WorldMessage {
    pub id: i64,
    pub sender: String,
    pub content: String,
    pub created_at: f64,
    pub mine: bool,
}

impl NativeDesktop {
    pub fn list_world_contacts(
        &self,
        keyword: &str,
        offset: i64,
    ) -> Result<(Vec<WorldContact>, i64)> {
        let (items, total) = self.state().projection.user_world.list_contacts(
            self.user_id(),
            (!keyword.trim().is_empty()).then_some(keyword.trim()),
            offset.max(0),
            100,
        )?;
        Ok((
            items
                .into_iter()
                .map(|item| WorldContact {
                    user_id: item.user_id,
                    username: item.username,
                    online: item.online,
                    conversation_id: item.conversation_id.unwrap_or_default(),
                    preview: item.last_message_preview.unwrap_or_default(),
                    unread: item.unread_count,
                })
                .collect(),
            total,
        ))
    }

    pub fn list_world_groups(&self, offset: i64) -> Result<(Vec<WorldGroup>, i64)> {
        let (items, total) =
            self.state()
                .projection
                .user_world
                .list_groups(self.user_id(), offset.max(0), 100)?;
        Ok((
            items
                .into_iter()
                .map(|item| WorldGroup {
                    group_id: item.group_id,
                    conversation_id: item.conversation_id,
                    name: item.group_name,
                    members: item.member_count,
                    preview: item.last_message_preview.unwrap_or_default(),
                    unread: item.unread_count_cache,
                })
                .collect(),
            total,
        ))
    }

    pub fn create_world_direct_conversation(&self, peer_user_id: &str) -> Result<String> {
        let item = self
            .state()
            .projection
            .user_world
            .resolve_or_create_direct_conversation(self.user_id(), peer_user_id, now())?;
        Ok(item.conversation_id)
    }

    pub fn create_world_group(&self, name: &str, member_user_ids: &[String]) -> Result<String> {
        let item = self.state().projection.user_world.create_group(
            self.user_id(),
            name,
            member_user_ids,
            now(),
        )?;
        Ok(item.conversation_id)
    }

    pub fn list_world_messages(&self, conversation_id: &str) -> Result<Vec<WorldMessage>> {
        let items = self.state().projection.user_world.list_messages(
            self.user_id(),
            conversation_id,
            None,
            100,
        )?;
        Ok(items
            .into_iter()
            .map(|item| WorldMessage {
                id: item.message_id,
                sender: item.sender_user_id.clone(),
                content: item.content,
                created_at: item.created_at,
                mine: item.sender_user_id == self.user_id(),
            })
            .collect())
    }

    pub fn mark_world_read(
        &self,
        conversation_id: &str,
        last_message_id: Option<i64>,
    ) -> Result<()> {
        self.runtime
            .block_on(self.state().projection.user_world.mark_read(
                self.user_id(),
                conversation_id,
                last_message_id,
                now(),
            ))?;
        Ok(())
    }

    pub fn send_world_message(&self, conversation_id: &str, content: &str) -> Result<WorldMessage> {
        let result = self
            .runtime
            .block_on(self.state().projection.user_world.send_message(
                self.user_id(),
                conversation_id,
                content,
                "text",
                None,
                now(),
            ))?;
        let item = result.message;
        Ok(WorldMessage {
            id: item.message_id,
            sender: item.sender_user_id.clone(),
            content: item.content,
            created_at: item.created_at,
            mine: item.sender_user_id == self.user_id(),
        })
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs_f64())
        .unwrap_or(0.0)
}
