use std::collections::HashSet;

use twin_core::lexical_canonical;

pub fn parse_mapped_libraries(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(path) = mapped_library_path(line) else {
            continue;
        };
        if seen.insert(path.clone()) {
            out.push(path);
        }
    }
    out
}

fn mapped_library_path(line: &str) -> Option<String> {
    let path = pathname_field(line.trim())?;
    if path.is_empty() || path.starts_with('[') || !path.starts_with('/') {
        return None;
    }
    let path = path.strip_suffix(" (deleted)").unwrap_or(path);
    let canonical = lexical_canonical(path);
    if canonical.starts_with('/') {
        Some(canonical)
    } else {
        None
    }
}

fn pathname_field(line: &str) -> Option<&str> {
    let mut rest = line;
    for _ in 0..5 {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(char::is_whitespace)?;
        rest = &rest[end..];
    }
    let path = rest.trim();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_absolute_libraries_and_skips_pseudo_entries() {
        let text = "\
55e0a-55e1b r-xp 00000000 08:01 1 /usr/sbin/nginx
7f000-7f200 r-xp 00000000 08:01 2 /usr/lib/libssl.so.3
7f200-7f400 r-xp 00020000 08:01 2 /usr/lib/libssl.so.3 (deleted)
7f400-7f500 rw-p 00000000 00:00 0 [heap]
7fff-8000 rw-p 00000000 00:00 0 [stack]
7f600-7f700 r-xp 00000000 00:00 0 [vdso]
7f700-7f800 r-xp 00000000 00:00 0 [vvar]
7f800-7f900 r-xp 00000000 00:00 0 [vsyscall]
this is not a maps line
7f900-7fa00 r-xp 00000000 08:01 3 relative.so
";
        let paths = parse_mapped_libraries(text);
        assert_eq!(
            paths,
            vec![
                "/usr/sbin/nginx".to_string(),
                "/usr/lib/libssl.so.3".to_string(),
            ]
        );
    }

    #[test]
    fn empty_and_short_lines_are_skipped() {
        assert!(parse_mapped_libraries("\n\nonly-three fields here\n").is_empty());
    }
}
