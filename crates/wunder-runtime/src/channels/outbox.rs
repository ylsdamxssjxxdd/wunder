use crate::config::ChannelOutboxConfig;

pub fn resolve_outbox_config(mut cfg: ChannelOutboxConfig) -> ChannelOutboxConfig {
    if cfg.poll_interval_ms == 0 {
        cfg.poll_interval_ms = 500;
    }
    if cfg.max_batch == 0 {
        cfg.max_batch = 50;
    }
    if cfg.max_retries == 0 {
        cfg.max_retries = 5;
    }
    if cfg.retry_base_s <= 0.0 {
        cfg.retry_base_s = 2.0;
    }
    if cfg.retry_max_s <= 0.0 {
        cfg.retry_max_s = 60.0;
    }
    cfg
}

pub fn compute_retry_at(now: f64, retry_count: i64, cfg: &ChannelOutboxConfig) -> f64 {
    let base = cfg.retry_base_s.max(1.0);
    let exponent = retry_count.max(1) as u32;
    let delay = base * 2_f64.powi(exponent as i32);
    let capped = delay.min(cfg.retry_max_s.max(base));
    now + capped
}
