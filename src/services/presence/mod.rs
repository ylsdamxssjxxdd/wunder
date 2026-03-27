mod connection;
mod watch;

pub use connection::UserPresenceView;
pub use watch::{ProjectionTargetKind, ProjectionWatchMetrics, ProjectionWatchView};

use connection::ConnectionPresenceService;
use std::collections::HashMap;
use watch::ProjectionWatchService;

pub struct PresenceService {
    connections: ConnectionPresenceService,
    watches: ProjectionWatchService,
}

impl PresenceService {
    pub fn new() -> Self {
        Self {
            connections: ConnectionPresenceService::new(),
            watches: ProjectionWatchService::new(),
        }
    }

    pub fn touch_user(&self, user_id: &str, now: f64) {
        self.connections.touch(user_id, now);
    }

    pub fn connect_client(&self, user_id: &str, connection_id: &str, now: f64) {
        self.connections.connect(user_id, connection_id, now);
    }

    pub fn disconnect_client(&self, user_id: &str, connection_id: &str, now: f64) {
        self.watches.disconnect_connection(connection_id);
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

    pub fn watch_projection(
        &self,
        connection_id: &str,
        request_id: &str,
        user_id: &str,
        target_kind: ProjectionTargetKind,
        target_id: &str,
        now: f64,
    ) {
        self.watches.watch(
            connection_id,
            request_id,
            user_id,
            target_kind,
            target_id,
            now,
        );
    }

    pub fn unwatch_projection(&self, connection_id: &str, request_id: &str) {
        self.watches.unwatch(connection_id, request_id);
    }

    pub fn projection_watch_snapshot(
        &self,
        target_kind: ProjectionTargetKind,
        target_id: &str,
        now: f64,
    ) -> Option<ProjectionWatchView> {
        self.watches.snapshot(target_kind, target_id, now)
    }

    pub fn projection_watch_metrics(&self, now: f64) -> ProjectionWatchMetrics {
        self.watches.metrics(now)
    }
}

impl Default for PresenceService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{PresenceService, ProjectionTargetKind};

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

    #[test]
    fn projection_watches_track_distinct_users_and_cleanup_on_disconnect() {
        let service = PresenceService::new();
        service.connect_client("alice", "conn-a", 10.0);
        service.connect_client("bob", "conn-b", 10.0);
        service.watch_projection(
            "conn-a",
            "req-1",
            "alice",
            ProjectionTargetKind::BeeroomGroup,
            "group-1",
            11.0,
        );
        service.watch_projection(
            "conn-a",
            "req-2",
            "alice",
            ProjectionTargetKind::BeeroomGroup,
            "group-1",
            12.0,
        );
        service.watch_projection(
            "conn-b",
            "req-1",
            "bob",
            ProjectionTargetKind::BeeroomGroup,
            "group-1",
            12.0,
        );

        let snapshot = service
            .projection_watch_snapshot(ProjectionTargetKind::BeeroomGroup, "group-1", 13.0)
            .expect("watch snapshot should exist");
        assert_eq!(snapshot.watch_count, 3);
        assert_eq!(snapshot.user_count, 2);

        service.unwatch_projection("conn-a", "req-2");
        let snapshot = service
            .projection_watch_snapshot(ProjectionTargetKind::BeeroomGroup, "group-1", 14.0)
            .expect("watch snapshot should still exist");
        assert_eq!(snapshot.watch_count, 2);
        assert_eq!(snapshot.user_count, 2);

        service.disconnect_client("alice", "conn-a", 15.0);
        let snapshot = service
            .projection_watch_snapshot(ProjectionTargetKind::BeeroomGroup, "group-1", 16.0)
            .expect("bob watch should remain");
        assert_eq!(snapshot.watch_count, 1);
        assert_eq!(snapshot.user_count, 1);
    }
}
