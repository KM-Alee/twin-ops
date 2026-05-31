use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessWarningKind {
    Vanished,
    PermissionDenied,
    Malformed,
    ExeUnreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessWarning {
    kind: ProcessWarningKind,
    path: PathBuf,
    detail: String,
}

impl ProcessWarning {
    pub(crate) fn new(kind: ProcessWarningKind, path: PathBuf, detail: impl Into<String>) -> Self {
        Self {
            kind,
            path,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> ProcessWarningKind {
        self.kind
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}
