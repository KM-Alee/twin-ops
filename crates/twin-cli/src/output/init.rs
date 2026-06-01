use twin_app::InitResult;

use crate::output::format::{Lines, Status};

pub fn render(result: &InitResult) -> String {
    let (status, detail) = init_status(result);
    let mut out = Lines::new();
    out.title("twin init");
    out.status_row(status, "result", detail);
    out.blank();
    out.section("layout");
    out.path_row(false, "config", &result.config_path, None);
    out.path_row(
        false,
        "database",
        &result.db_path,
        Some(&format!("schema v{}", result.schema_version)),
    );
    out.path_row(true, "log", &result.log_path, None);
    out.into_string()
}

fn init_status(result: &InitResult) -> (Status, &'static str) {
    if result.config_created || result.db_created {
        (Status::Ok, "created")
    } else if result.config_updated {
        (Status::Warn, "updated")
    } else {
        (Status::Neutral, "unchanged")
    }
}
