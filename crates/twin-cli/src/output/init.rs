use twin_app::InitResult;

pub fn render(result: &InitResult) -> String {
    let status = if result.config_created || result.db_created {
        "Created"
    } else if result.config_updated {
        "Updated"
    } else {
        "Already exists"
    };
    format!(
        "Twin init: {status}\n\n\
         Config: {}\n\
         Database: {}\n\
         Log: {}",
        result.config_path.display(),
        result.db_path.display(),
        result.log_path.display(),
    )
}
