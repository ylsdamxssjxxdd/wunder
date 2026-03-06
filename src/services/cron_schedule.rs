use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};

pub const MIN_EVERY_MS: i64 = 1_000;
pub const MAX_EVERY_MS: i64 = 86_400_000;
pub const MAX_AT_HORIZON_SECS: i64 = 365 * 24 * 3600;
pub const MAX_NAME_LEN: usize = 128;
pub const MAX_MESSAGE_LEN: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedScheduleText {
    EveryMs(i64),
    Cron(String),
}

pub fn parse_schedule_text(input: &str) -> Result<ParsedScheduleText> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(anyhow!("schedule text empty"));
    }
    let lowered = raw.to_lowercase();

    if is_cron_expr(raw) {
        return Ok(ParsedScheduleText::Cron(raw.to_string()));
    }

    if let Some(parsed) = parse_every_expression(&lowered)? {
        return Ok(ParsedScheduleText::EveryMs(parsed));
    }

    if let Some(expr) = parse_daily_weekly(&lowered)? {
        return Ok(ParsedScheduleText::Cron(expr));
    }

    if let Some(expr) = parse_simple_keywords(&lowered)? {
        return Ok(ParsedScheduleText::Cron(expr));
    }

    Err(anyhow!(
        "unsupported schedule text: {raw}. Try 'every 5 minutes', 'daily at 9am', 'weekdays at 6pm', or a cron expression like '0 */5 * * *'"
    ))
}

pub fn validate_cron_expr(expr: &str) -> Result<()> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("cron expression must not be empty"));
    }
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if !(5..=7).contains(&fields.len()) {
        return Err(anyhow!(
            "cron expression must have 5-7 fields (got {}): \"{}\"",
            fields.len(),
            trimmed
        ));
    }
    for (idx, field) in fields.iter().enumerate() {
        if field.is_empty() {
            return Err(anyhow!("cron field {idx} is empty"));
        }
        if !field
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | '-' | ',' | '?'))
        {
            return Err(anyhow!(
                "cron field {idx} has unsupported characters: \"{}\"",
                field
            ));
        }
    }
    Ok(())
}

pub fn normalize_every_ms(value: i64) -> Result<i64> {
    let ms = value.max(MIN_EVERY_MS);
    if ms > MAX_EVERY_MS {
        return Err(anyhow!(
            "schedule.every_ms too large ({ms}, max {MAX_EVERY_MS})"
        ));
    }
    if ms <= 0 {
        return Err(anyhow!("schedule.every_ms must be positive"));
    }
    Ok(ms)
}

pub fn validate_schedule_at(at: DateTime<Utc>, now: DateTime<Utc>) -> Result<()> {
    if at <= now {
        return Err(anyhow!("schedule.at must be in the future"));
    }
    let delta = (at - now).num_seconds();
    if delta > MAX_AT_HORIZON_SECS {
        return Err(anyhow!(
            "schedule.at too far in the future (max {MAX_AT_HORIZON_SECS}s)"
        ));
    }
    Ok(())
}

pub fn validate_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("name must not be empty"));
    }
    if trimmed.chars().count() > MAX_NAME_LEN {
        return Err(anyhow!(
            "name too long ({} chars, max {MAX_NAME_LEN})",
            trimmed.chars().count()
        ));
    }
    Ok(())
}

pub fn validate_message(message: &str) -> Result<()> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("payload.message must not be empty"));
    }
    if trimmed.chars().count() > MAX_MESSAGE_LEN {
        return Err(anyhow!(
            "payload.message too long ({} chars, max {MAX_MESSAGE_LEN})",
            trimmed.chars().count()
        ));
    }
    Ok(())
}

fn is_cron_expr(input: &str) -> bool {
    let trimmed = input.trim();
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if !(5..=7).contains(&fields.len()) {
        return false;
    }
    fields.iter().all(|field| {
        !field.is_empty()
            && field
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | '-' | ',' | '?'))
    })
}

fn parse_every_expression(input: &str) -> Result<Option<i64>> {
    let Some(rest) = input.strip_prefix("every ") else {
        return Ok(None);
    };
    let rest = rest.trim();
    if rest == "second" || rest == "1 second" {
        return Ok(Some(1_000));
    }
    if rest == "minute" || rest == "1 minute" {
        return Ok(Some(60_000));
    }
    if rest == "hour" || rest == "1 hour" {
        return Ok(Some(3_600_000));
    }
    if rest == "day" || rest == "1 day" {
        return Ok(Some(86_400_000));
    }
    if let Some(value) = parse_unit_suffix(rest, "seconds", 1_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "second", 1_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "secs", 1_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "sec", 1_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "minutes", 60_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "minute", 60_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "mins", 60_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "min", 60_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "hours", 3_600_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "hour", 3_600_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "days", 86_400_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "day", 86_400_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "s", 1_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "m", 60_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "h", 3_600_000)? {
        return Ok(Some(value));
    }
    if let Some(value) = parse_unit_suffix(rest, "d", 86_400_000)? {
        return Ok(Some(value));
    }
    Ok(None)
}

fn parse_unit_suffix(input: &str, suffix: &str, unit_ms: i64) -> Result<Option<i64>> {
    let Some(value) = input.strip_suffix(suffix) else {
        return Ok(None);
    };
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let parsed: i64 = raw
        .parse()
        .map_err(|_| anyhow!("invalid schedule number: {raw}"))?;
    if parsed <= 0 {
        return Err(anyhow!("schedule value must be positive"));
    }
    Ok(Some(parsed.saturating_mul(unit_ms)))
}

fn parse_daily_weekly(input: &str) -> Result<Option<String>> {
    if let Some(time_str) = input.strip_prefix("daily at ") {
        let (hour, minute) = parse_time_to_hour_min(time_str)?;
        return Ok(Some(format!("{minute} {hour} * * *")));
    }
    if let Some(time_str) = input.strip_prefix("weekdays at ") {
        let (hour, minute) = parse_time_to_hour_min(time_str)?;
        return Ok(Some(format!("{minute} {hour} * * 1-5")));
    }
    if let Some(time_str) = input.strip_prefix("weekends at ") {
        let (hour, minute) = parse_time_to_hour_min(time_str)?;
        return Ok(Some(format!("{minute} {hour} * * 0,6")));
    }
    Ok(None)
}

fn parse_simple_keywords(input: &str) -> Result<Option<String>> {
    let expr = match input {
        "hourly" => "0 * * * *",
        "daily" => "0 0 * * *",
        "weekly" => "0 0 * * 0",
        "monthly" => "0 0 1 * *",
        _ => return Ok(None),
    };
    Ok(Some(expr.to_string()))
}

fn parse_time_to_hour_min(value: &str) -> Result<(u32, u32)> {
    let mut s = value.trim().to_lowercase();
    let mut is_pm = false;
    let mut is_am = false;
    if let Some(rest) = s.strip_suffix("am") {
        is_am = true;
        s = rest.trim().to_string();
    } else if let Some(rest) = s.strip_suffix("pm") {
        is_pm = true;
        s = rest.trim().to_string();
    }

    let (hour, minute) = if let Some((h, m)) = s.split_once(':') {
        let hour: u32 = h.trim().parse().map_err(|_| anyhow!("invalid time: {value}"))?;
        let minute: u32 = m.trim().parse().map_err(|_| anyhow!("invalid time: {value}"))?;
        (hour, minute)
    } else {
        let hour: u32 = s.trim().parse().map_err(|_| anyhow!("invalid time: {value}"))?;
        (hour, 0)
    };

    if minute > 59 {
        return Err(anyhow!("invalid minute: {minute}"));
    }

    let hour = if is_am || is_pm {
        if hour == 0 || hour > 12 {
            return Err(anyhow!("invalid hour: {hour}"));
        }
        match (is_am, is_pm, hour) {
            (true, _, 12) => 0,
            (_, true, 12) => 12,
            (true, _, _) => hour,
            (_, true, _) => hour + 12,
            _ => hour,
        }
    } else {
        if hour > 23 {
            return Err(anyhow!("invalid hour: {hour}"));
        }
        hour
    };

    Ok((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_schedule_text_every() {
        assert_eq!(
            parse_schedule_text("every 5 minutes").unwrap(),
            ParsedScheduleText::EveryMs(300_000)
        );
        assert_eq!(
            parse_schedule_text("every 2h").unwrap(),
            ParsedScheduleText::EveryMs(7_200_000)
        );
    }

    #[test]
    fn test_parse_schedule_text_daily() {
        assert_eq!(
            parse_schedule_text("daily at 9am").unwrap(),
            ParsedScheduleText::Cron("0 9 * * *".to_string())
        );
        assert_eq!(
            parse_schedule_text("weekdays at 6:30pm").unwrap(),
            ParsedScheduleText::Cron("30 18 * * 1-5".to_string())
        );
    }

    #[test]
    fn test_parse_schedule_text_cron() {
        assert_eq!(
            parse_schedule_text("0 */5 * * *").unwrap(),
            ParsedScheduleText::Cron("0 */5 * * *".to_string())
        );
    }

    #[test]
    fn test_validate_cron_expr() {
        assert!(validate_cron_expr("0 */5 * * *").is_ok());
        assert!(validate_cron_expr("bad cron").is_err());
    }
}
