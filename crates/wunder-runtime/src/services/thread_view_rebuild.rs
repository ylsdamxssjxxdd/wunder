//! Fair, bounded background migration. A page open never starts a rebuild.
use crate::{core::blocking, storage::StorageBackend};
use std::sync::Arc;

pub fn start(storage: Arc<dyn StorageBackend>) {
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    tokio::spawn(async move {
        loop {
            let db = storage.clone();
            let pending = blocking::run_db("thread_view.rebuild.pending", move || {
                db.pending_thread_view_rebuilds()
            })
            .await
            .unwrap_or_default();
            let idle = pending.is_empty();
            for (user, session, after) in pending {
                let db = storage.clone();
                if let Err(error) = blocking::run_db("thread_view.rebuild.page", move || {
                    db.rebuild_thread_view_page(&user, &session, after)
                })
                .await
                {
                    tracing::warn!(%error,"thread view rebuild deferred");
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(if idle {
                5000
            } else {
                20
            }))
            .await;
        }
    });
}
