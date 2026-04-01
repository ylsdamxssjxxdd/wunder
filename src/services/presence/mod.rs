mod connection;

pub use connection::UserPresenceView;

use connection::ConnectionPresenceService;
use std::collections::HashMap;

pub struct PresenceService {
    connections: ConnectionPresenceService,
}

impl PresenceService {
    pub fn new() -> Self {
        Self {
            connections: ConnectionPresenceService::new(),
        }
    }

    pub fn touch_user(&self, user_id: &str, now: f64) {
        self.connections.touch(user_id, now);
    }

    pub fn connect_client(&self, user_id: &str, connection_id: &str, now: f64) {
        self.connections.connect(user_id, connection_id, now);
    }

    pub fn disconnect_client(&self, user_id: &str, connection_id: &str, now: f64) {
        self.connections.disconnect(user_id, connection_id, now);
    }

    pub fn user_snapshot(&self, user_id: &str, now: f64) -> Option<UserPresenceView> {
        self.connections.snapshot(user_id, now)
    }

    pub fn user_snapshot_many<I, S>(
        &self,
        user_ids: I,
        now: f64,
    ) -> HashMap<String, UserPresenceView>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.connections.snapshot_many(user_ids, now)
    }
}

impl Default for PresenceService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::PresenceService;

    #[test]
    fn client_connections_are_counted_by_connection_id() {
        let service = PresenceService::new();
        service.connect_client("alice", "conn-1", 10.0);
        service.connect_client("alice", "conn-1", 12.0);
        service.connect_client("alice", "conn-2", 13.0);
        let snapshot = service
            .user_snapshot("alice", 14.0)
            .expect("presence should exist");
        assert!(snapshot.online);
        assert_eq!(snapshot.connection_count, 2);
        assert_eq!(snapshot.last_seen_at, 13.0);
        service.disconnect_client("alice", "conn-1", 20.0);
        let snapshot = service
            .user_snapshot("alice", 21.0)
            .expect("presence should still exist");
        assert_eq!(snapshot.connection_count, 1);
        service.disconnect_client("alice", "conn-2", 22.0);
        let snapshot = service
            .user_snapshot("alice", 23.0)
            .expect("presence should remain during ttl");
        assert_eq!(snapshot.connection_count, 0);
        assert!(snapshot.online);
    }
}
