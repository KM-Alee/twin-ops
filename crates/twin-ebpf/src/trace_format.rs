use crate::EbpfError;
use std::path::Path;

pub(crate) fn validate_host() -> Result<(), EbpfError> {
    if cfg!(not(all(
        target_os = "linux",
        target_endian = "little",
        target_pointer_width = "64"
    ))) {
        return Err(unavailable(
            "bundled tracepoint ABI requires little-endian 64-bit Linux",
        ));
    }
    let format = read_format("sock/inet_sock_set_state")?;
    validate(&format)?;
    for name in ["accept", "accept4", "bind"] {
        validate_fields(
            &read_format(&format!("syscalls/sys_enter_{name}"))?,
            &[("fd", 16, 8)],
        )?;
        validate_fields(
            &read_format(&format!("syscalls/sys_exit_{name}"))?,
            &[("ret", 16, 8)],
        )?;
    }
    Ok(())
}
fn read_format(trace: &str) -> Result<String, EbpfError> {
    [
        "/sys/kernel/tracing/events",
        "/sys/kernel/debug/tracing/events",
    ]
    .into_iter()
    .find_map(|root| std::fs::read_to_string(Path::new(root).join(trace).join("format")).ok())
    .ok_or_else(|| unavailable("TCP tracepoint format unreadable"))
}
fn unavailable(reason: &str) -> EbpfError {
    EbpfError::Unavailable {
        reason: reason.into(),
    }
}
fn validate(format: &str) -> Result<(), EbpfError> {
    validate_fields(
        format,
        &[
            ("skaddr", 8, 8),
            ("oldstate", 16, 4),
            ("newstate", 20, 4),
            ("sport", 24, 2),
            ("dport", 26, 2),
            ("family", 28, 2),
            ("protocol", 30, 2),
            ("saddr", 32, 4),
            ("daddr", 36, 4),
            ("saddr_v6", 40, 16),
            ("daddr_v6", 56, 16),
        ],
    )
}
fn validate_fields(format: &str, expected: &[(&str, usize, usize)]) -> Result<(), EbpfError> {
    for &(name, offset, size) in expected {
        let is_valid = format.lines().any(|line| {
            let fields: Vec<_> = line.split(';').map(str::trim).collect();
            let declaration = fields.first().copied().unwrap_or("");
            let actual_name = declaration
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_start_matches('*')
                .split('[')
                .next()
                .unwrap_or("");
            actual_name == name
                && fields
                    .iter()
                    .any(|field| *field == format!("offset:{offset}"))
                && fields.iter().any(|field| *field == format!("size:{size}"))
        });
        if !is_valid {
            return Err(unavailable(
                "TCP tracepoint format does not match bundled ABI",
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_offsets_and_missing_fields_fail_closed() {
        let format = [
            ("skaddr", 8, 8),
            ("oldstate", 16, 4),
            ("newstate", 20, 4),
            ("sport", 24, 2),
            ("dport", 26, 2),
            ("family", 28, 2),
            ("protocol", 30, 2),
            ("saddr", 32, 4),
            ("daddr", 36, 4),
            ("saddr_v6", 40, 16),
            ("daddr_v6", 56, 16),
        ]
        .into_iter()
        .map(|(name, offset, size)| {
            format!("field:unsigned long {name}; offset:{offset}; size:{size}; signed:0;\n")
        })
        .collect::<String>();
        assert!(validate(&format).is_ok());
        assert!(validate(&format.replace("offset:56", "offset:60")).is_err());
        assert!(validate("").is_err());
    }
}
