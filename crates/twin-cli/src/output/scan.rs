use twin_app::{ScanResult, ScanWarning};

pub fn render(result: &ScanResult) -> String {
    let mut lines = vec![
        "Scan complete.".to_string(),
        String::new(),
        "Nodes:".to_string(),
        format!("- processes: {}", result.process_count),
        String::new(),
        "Edges:".to_string(),
        format!("- parent-child: {}", result.parent_edge_count),
    ];

    if result.warning_count > 0 {
        lines.push(String::new());
        lines.push("Warnings:".to_string());
        for warning in &result.warnings {
            lines.push(format!("- {}", warning_line(warning)));
        }
    }

    lines.join("\n")
}

fn warning_line(warning: &ScanWarning) -> String {
    match warning.kind.as_str() {
        "vanished_processes" => format!("{} processes disappeared during scan", warning.count),
        "exe_unreadable" => format!("{} process exe links unreadable", warning.count),
        "permission_denied" => format!("{} processes permission denied", warning.count),
        "malformed_proc_files" => format!("{} malformed proc files", warning.count),
        other => format!("{} {}", warning.count, other.replace('_', " ")),
    }
}
