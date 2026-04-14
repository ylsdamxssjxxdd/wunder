use axum::extract::ws::Message;
use dashmap::DashMap;
use serde_json::json;
use tokio::sync::mpsc;

const FORCED_LOGOUT_CODE: &str = "SESSION_REPLACED";
const FORCED_LOGOUT_MESSAGE: &str = "session replaced by a newer login";

#[derive(Clone)]
struct AuthConnection {
    user_id: String,
    session_scope: String,
    sender: mpsc::Sender<Message>,
}

pub struct AuthSessionService {
    connections: DashMap<String, AuthConnection>,
}

impl AuthSessionService {
    pub fn new() -> Self {
        Self {
            connections: DashMap::new(),
        }
    }

    pub fn register(
        &self,
        user_id: &str,
        session_scope: &str,
        connection_id: &str,
        sender: mpsc::Sender<Message>,
    ) {
        let normalized_user_id = user_id.trim();
        let normalized_session_scope = session_scope.trim();
        let normalized_connection_id = connection_id.trim();
        if normalized_user_id.is_empty()
            || normalized_session_scope.is_empty()
            || normalized_connection_id.is_empty()
        {
            return;
        }
        self.connections.insert(
            normalized_connection_id.to_string(),
            AuthConnection {
                user_id: normalized_user_id.to_string(),
                session_scope: normalized_session_scope.to_string(),
                sender,
            },
        );
    }

    pub fn unregister(&self, connection_id: &str) {
        let normalized_connection_id = connection_id.trim();
        if normalized_connection_id.is_empty() {
            return;
        }
        self.connections.remove(normalized_connection_id);
    }

    pub async fn force_logout_user(&self, user_id: &str, session_scope: &str) {
        let normalized_user_id = user_id.trim();
        let normalized_session_scope = session_scope.trim();
        if normalized_user_id.is_empty() || normalized_session_scope.is_empty() {
            return;
        }

        let targets = self
            .connections
            .iter()
            .filter_map(|entry| {
                let connection_id = entry.key().trim();
                let connection = entry.value();
                if connection.user_id != normalized_user_id
                    || connection.session_scope != normalized_session_scope
                    || connection_id.is_empty()
                {
                    return None;
                }
                Some((connection_id.to_string(), connection.sender.clone()))
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return;
        }

        let error_text = build_forced_logout_payload();
        for (connection_id, sender) in targets {
            let forced_logout_error = error_text.clone();
            tokio::spawn(async move {
                let _ = sender.send(Message::Text(forced_logout_error.into())).await;
                let _ = sender.send(Message::Close(None)).await;
            });
            self.connections.remove(&connection_id);
        }
    }
}

fn build_forced_logout_payload() -> String {
    json!({
        "type": "error",
        "payload": {
            "status": 401,
            "code": FORCED_LOGOUT_CODE,
            "message": FORCED_LOGOUT_MESSAGE,
            "hint": "Sign in again to continue.",
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::AuthSessionService;
    use axum::extract::ws::Message;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn force_logout_user_pushes_error_and_close() {
        let service = AuthSessionService::new();
        let (tx, mut rx) = mpsc::channel::<Message>(4);
        service.register("alice", "user_web", "ws_1", tx);

        service.force_logout_user("alice", "user_web").await;

        let first = rx.recv().await.expect("forced logout error");
        let second = rx.recv().await.expect("forced logout close");
        match first {
            Message::Text(text) => {
                let payload: serde_json::Value =
                    serde_json::from_str(&text).expect("parse forced logout payload");
                assert_eq!(
                    payload.get("type").and_then(serde_json::Value::as_str),
                    Some("error")
                );
                assert_eq!(
                    payload
                        .get("payload")
                        .and_then(|value| value.get("code"))
                        .and_then(serde_json::Value::as_str),
                    Some("SESSION_REPLACED")
                );
            }
            other => panic!("unexpected first message: {other:?}"),
        }
        assert!(matches!(second, Message::Close(_)));
    }

    #[tokio::test]
    async fn force_logout_user_keeps_other_scope_connections() {
        let service = AuthSessionService::new();
        let (user_tx, mut user_rx) = mpsc::channel::<Message>(4);
        let (admin_tx, mut admin_rx) = mpsc::channel::<Message>(4);
        service.register("alice", "user_web", "ws_1", user_tx);
        service.register("alice", "admin_web", "ws_2", admin_tx);

        service.force_logout_user("alice", "user_web").await;

        assert!(matches!(user_rx.recv().await, Some(Message::Text(_))));
        assert!(matches!(user_rx.recv().await, Some(Message::Close(_))));
        assert!(admin_rx.try_recv().is_err());
    }
}
