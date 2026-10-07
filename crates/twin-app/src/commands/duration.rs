use twin_core::TimestampNs;

use crate::error::TemporalError;

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
    let secs = parse_duration_secs(input)?;
    let since = now
        .as_i64()
        .saturating_sub((secs as i64).saturating_mul(1_000_000_000));
    Ok(since)
}

pub(crate) fn parse_duration_secs(input: &str) -> Result<u64, TemporalError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(TemporalError::InvalidDuration {
            value: input.to_string(),
            reason: "duration cannot be empty".to_string(),
        });
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
    Ok(secs)
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
