use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct CoverageReport {
    pub readable_processes: usize,
    pub restricted_processes: usize,
    pub unmapped_sockets: usize,
    pub unavailable_collectors: Vec<String>,
    pub ebpf_available: bool,
    pub docker_available: bool,
    pub kubernetes_available: bool,
}

impl CoverageReport {
    pub fn unknown_lines(&self) -> Vec<CoverageUnknown> {
        let mut lines = Vec::new();
        if self.restricted_processes > 0 {
            lines.push(CoverageUnknown {
                kind: "permission_gap",
                detail: format!(
                    "{} processes hidden due to permissions",
                    self.restricted_processes
                ),
            });
        }
        if self.unmapped_sockets > 0 {
            lines.push(CoverageUnknown {
                kind: "unmapped_sockets",
                detail: format!("{} sockets could not be mapped", self.unmapped_sockets),
            });
        }
        for name in &self.unavailable_collectors {
            lines.push(CoverageUnknown {
                kind: "unavailable_collector",
                detail: format!("{name} collector unavailable"),
            });
        }
        lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageUnknown {
    pub kind: &'static str,
    pub detail: String,
}
