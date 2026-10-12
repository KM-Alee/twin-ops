use std::fs;
use std::path::Path;

use crate::model::DoctorCore;

pub fn check(config_path: &Path) -> DoctorCore {
    let config_found = fs::metadata(config_path).is_ok();
    let config_error = if config_found {
        crate::read_config(config_path)
            .err()
            .map(|error| error.to_string())
    } else {
        None
    };
    DoctorCore {
        cli_ok: true,
        config_found,
        config_path: Some(config_path.to_path_buf()),
        config_error,
    }
}
