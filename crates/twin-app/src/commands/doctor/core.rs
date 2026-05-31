use std::fs;
use std::path::Path;

use crate::model::DoctorCore;

pub fn check(config_path: &Path) -> DoctorCore {
    DoctorCore {
        cli_ok: true,
        config_found: fs::metadata(config_path).is_ok(),
        config_path: Some(config_path.to_path_buf()),
    }
}
