use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Bad,
    Neutral,
}

pub struct Lines {
    inner: Vec<String>,
}

impl Default for Lines {
    fn default() -> Self {
        Self::new()
    }
}

impl Lines {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn title(&mut self, command: &str) {
        self.inner.push(command.to_string());
        self.inner.push("═".repeat(command.chars().count().max(8)));
        self.blank();
    }

    pub fn blank(&mut self) {
        self.inner.push(String::new());
    }

    pub fn section(&mut self, name: &str) {
        self.inner.push(name.to_string());
    }

    pub fn status_row(&mut self, status: Status, label: &str, detail: &str) {
        self.inner.push(format!(
            "  {:<12}  {}  {}",
            status_tag(status),
            label,
            detail
        ));
    }

    pub fn tree_leaf(&mut self, is_last: bool, label: &str, value: &str) {
        let branch = if is_last { "└──" } else { "├──" };
        self.inner
            .push(format!("  {branch}  {:<14}  {value}", label));
    }

    pub fn path_row(&mut self, is_last: bool, label: &str, path: &Path, note: Option<&str>) {
        let value = match note {
            Some(note) => format!("{}  ({note})", path.display()),
            None => path.display().to_string(),
        };
        self.tree_leaf(is_last, label, &value);
    }

    pub fn into_string(self) -> String {
        self.inner.join("\n")
    }
}

pub(crate) fn status_tag(status: Status) -> &'static str {
    match status {
        Status::Ok => "ok",
        Status::Warn => "warn",
        Status::Bad => "fail",
        Status::Neutral => "—",
    }
}

pub fn format_duration_ns(started_ns: i64, ended_ns: i64) -> String {
    let delta_ms = (ended_ns.saturating_sub(started_ns)) / 1_000_000;
    if delta_ms < 1000 {
        return format!("{delta_ms} ms");
    }
    let secs = delta_ms / 1000;
    let ms = delta_ms % 1000;
    format!("{secs}.{ms:03}s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_formats_milliseconds() {
        assert_eq!(format_duration_ns(0, 2_500_000), "2 ms");
    }
}
