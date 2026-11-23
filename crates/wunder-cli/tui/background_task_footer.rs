use std::time::{Duration, Instant};

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) struct BackgroundTaskFooter {
    summary: Option<String>,
    session_id: String,
    refreshed_at: Option<Instant>,
}

impl BackgroundTaskFooter {
    pub(crate) fn new(session_id: &str) -> Self {
        Self {
            summary: None,
            session_id: session_id.to_string(),
            refreshed_at: None,
        }
    }

    pub(crate) fn summary(
        &mut self,
        session_id: &str,
        is_zh: bool,
        count: impl FnOnce() -> usize,
    ) -> Option<String> {
        let session_changed = self.session_id != session_id;
        let fresh = self
            .refreshed_at
            .is_some_and(|at| at.elapsed() < REFRESH_INTERVAL);
        if !session_changed && fresh {
            return self.summary.clone();
        }
        self.summary = format_summary(count(), is_zh);
        self.session_id = session_id.to_string();
        self.refreshed_at = Some(Instant::now());
        self.summary.clone()
    }
}

fn format_summary(count: usize, is_zh: bool) -> Option<String> {
    if count == 0 {
        return None;
    }
    Some(if is_zh {
        format!("{count} 个后台任务运行中")
    } else {
        format!(
            "{count} background terminal{} running",
            if count == 1 { "" } else { "s" }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_is_compact_and_localized() {
        assert_eq!(format_summary(0, false), None);
        assert_eq!(
            format_summary(1, false).as_deref(),
            Some("1 background terminal running")
        );
        assert_eq!(
            format_summary(2, false).as_deref(),
            Some("2 background terminals running")
        );
        assert_eq!(
            format_summary(3, true).as_deref(),
            Some("3 个后台任务运行中")
        );
    }

    #[test]
    fn cache_is_scoped_to_the_current_thread() {
        let mut footer = BackgroundTaskFooter::new("thread-a");
        assert_eq!(
            footer.summary("thread-a", false, || 1).as_deref(),
            Some("1 background terminal running")
        );
        assert_eq!(
            footer.summary("thread-a", false, || 9).as_deref(),
            Some("1 background terminal running")
        );
        assert_eq!(
            footer.summary("thread-b", false, || 2).as_deref(),
            Some("2 background terminals running")
        );
    }
}
