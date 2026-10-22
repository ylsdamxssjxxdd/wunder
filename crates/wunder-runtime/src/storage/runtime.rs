//! Process-lifetime executor shared by canonical writers and sync facades.
//! A writer must not inherit a short-lived request or current-thread runtime:
//! blocking that caller to enter the sync facade would park its writer too.
use anyhow::{anyhow, Result};
use std::sync::OnceLock;

pub(crate) fn runtime() -> Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<std::result::Result<tokio::runtime::Runtime, String>> =
        OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .max_blocking_threads(16)
                .enable_all()
                .thread_name("canonical-storage")
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow!(error.clone()))
}
