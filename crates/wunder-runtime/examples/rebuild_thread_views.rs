//! Offline index preparation; reads configuration and never changes thread execution.
use anyhow::Result;
use wunder_server::{config::load_config, storage::build_storage};
fn main() -> Result<()> {
    let config = load_config();
    let storage = build_storage(&config.storage)?;
    storage.ensure_initialized()?;
    let mut pages = 0usize;
    loop {
        let pending = storage.pending_thread_view_rebuilds()?;
        if pending.is_empty() {
            break;
        }
        for (user, session, after) in pending {
            storage.rebuild_thread_view_page(&user, &session, after)?;
            pages += 1;
        }
        if pages % 100 == 0 {
            println!("display index batches completed: {pages}");
        }
    }
    println!("display index ready; batches: {pages}");
    Ok(())
}
