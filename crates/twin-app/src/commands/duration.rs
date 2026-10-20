use twin_core::TimestampNs;

use crate::error::{AppError, TemporalError};

const MAX_DURATION_SECS: u64 = 365 * 24 * 60 * 60;

pub fn parse_since_duration(input: &str, now: TimestampNs) -> Result<i64, TemporalError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration cannot be empty".to_string(),
        });
    }
    if trimmed.eq_ignore_ascii_case("yesterday") {
        let since = now.as_i64().saturating_sub(24 * 60 * 60 * 1_000_000_000);
        return Ok(since);
    }
    let (num_str, unit) = split_duration(trimmed)?;
    let amount: u64 = num_str
        .parse()
        .map_err(|_| TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration amount must be a positive integer".to_string(),
        })?;
    if amount == 0 {
        return Err(TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration must be greater than zero".to_string(),
        });
    }
    let secs = match unit {
        's' => amount,
        'm' => amount.saturating_mul(60),
        'h' => amount.saturating_mul(60 * 60),
        'd' => amount.saturating_mul(24 * 60 * 60),
        _ => {
            return Err(TemporalError::InvalidDuration {
                value: input.to_string(),
                reason: "unknown duration unit; use s, m, h, or d".to_string(),
            });
        }
    };
    if secs > MAX_DURATION_SECS {
        return Err(TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration is unreasonably large".to_string(),
        });
    }
    let since = now
        .as_i64()
        .saturating_sub((secs as i64).saturating_mul(1_000_000_000));
    Ok(since)
}

pub(crate) fn clock_duration(input: &str) -> Result<std::time::Duration, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("duration cannot be empty".to_string());
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return secs_from_amount(trimmed, 1);
    }
    let (num, unit) = split_duration(trimmed).map_err(|err| match err {
        TemporalError::InvalidDuration { reason, .. } => reason,
        other => other.to_string(),
    })?;
    let factor = match unit.to_ascii_lowercase() {
        's' => 1u64,
        'm' => 60,
        'h' => 60 * 60,
        'd' => 24 * 60 * 60,
        _ => return Err("unknown duration unit; use s, m, h, or d".to_string()),
    };
    secs_from_amount(num, factor)
}

fn secs_from_amount(num_str: &str, factor: u64) -> Result<std::time::Duration, String> {
    let amount: u64 = num_str
        .parse()
        .map_err(|_| "duration amount must be a positive integer".to_string())?;
    if amount == 0 {
        return Err("duration must be greater than zero".to_string());
    }
    let secs = amount
        .checked_mul(factor)
        .ok_or_else(|| "duration is unreasonably large".to_string())?;
    if secs > MAX_DURATION_SECS {
        return Err("duration is unreasonably large".to_string());
    }
    Ok(std::time::Duration::from_secs(secs))
}

fn split_duration(input: &str) -> Result<(&str, char), TemporalError> {
    let mut chars = input.chars();
    let last = chars
        .next_back()
        .ok_or_else(|| TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration cannot be empty".to_string(),
        })?;
    if !last.is_ascii_alphabetic() {
        return Err(TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration must end with a unit suffix (s, m, h, d)".to_string(),
        });
    }
    let num = input
        .get(..input.len().saturating_sub(last.len_utf8()))
        .unwrap_or("");
    if num.is_empty() {
        return Err(TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration amount missing".to_string(),
        });
    }
    Ok((num, last))
}

pub fn parse_graph_time(raw: &str) -> Result<twin_core::TimestampNs, AppError> {
    let invalid = || TemporalError::InvalidDuration {
        value: raw.into(),
        reason: "expected RFC3339 time or elapsed duration such as 1h".into(),
    };
    let ns = if raw == "now" {
        twin_core::TimestampNs::now().as_i64()
    } else if raw.contains('T') || raw.contains('t') {
        chrono::DateTime::parse_from_rfc3339(raw)
            .map_err(|_| invalid())?
            .timestamp_nanos_opt()
            .ok_or_else(invalid)?
    } else {
        parse_since_duration(raw, twin_core::TimestampNs::now())?
    };
    if ns < 0 || ns > twin_core::TimestampNs::now().as_i64() {
        return Err(invalid().into());
    }
    Ok(twin_core::TimestampNs::new(ns))
}

/// A coarse human age such as `40 s`, `12 min`, `5 h` or `3 d`.
pub(crate) fn age_label(elapsed_ns: i64) -> String {
    let seconds = elapsed_ns.max(0) / 1_000_000_000;
    match seconds {
        0..=59 => format!("{seconds} s"),
        60..=3_599 => format!("{} min", seconds / 60),
        3_600..=86_399 => format!("{} h", seconds / 3_600),
        _ => format!("{} d", seconds / 86_400),
    }
}
