mod document;
mod error;
mod evaluate;
mod starter;

pub use document::{parse_file, parse_str, CheckKind, EmulateAction, TestCheck, TestDocument};
pub use error::TestError;
pub use evaluate::{
    evaluate, CheckResult, CheckStatus, DiskFact, EdgeFact, EmulationFact, EndpointFact,
    GraphFacts, NodeFact, TestRunReport, UnknownFact,
};
pub use starter::STARTER_YAML;

use std::fs;
use std::path::Path;

pub fn write_starter(path: &Path, force: bool) -> Result<(), TestError> {
    if path.exists() && !force {
        return Err(TestError::AlreadyExists {
            path: path.to_path_buf(),
        });
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|source| TestError::Write {
                path: path.to_path_buf(),
                source,
            })?;
        }
    }
    fs::write(path, STARTER_YAML).map_err(|source| TestError::Write {
        path: path.to_path_buf(),
        source,
    })
}
