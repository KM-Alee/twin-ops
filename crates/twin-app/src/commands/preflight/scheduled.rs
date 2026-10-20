use super::intent::{classify_line, ContainerVerb, Intent, ServiceVerb};

const FACT_CAP: usize = 8;

/// What a scheduled command runs and changes, reduced to typed facts. Arguments are never
/// kept: they can carry credentials.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ScheduledFacts {
    pub programs: Vec<String>,
    pub operations: Vec<String>,
}

pub(crate) fn scheduled_facts(command: &str) -> ScheduledFacts {
    let mut facts = ScheduledFacts::default();
    for (_, intent) in classify_line(command) {
        let (program, operation) = match intent {
            Intent::ReadOnly { tool } | Intent::Unrecognized { tool } => (Some(tool), None),
            Intent::RunScript { path } => (Some(path), None),
            Intent::Service { verb, unit } => (
                Some("systemctl".into()),
                Some(format!("{} {}", service_verb(verb), unit_name(&unit))),
            ),
            Intent::Container { verb, name } => (
                Some("docker".into()),
                Some(format!("{} container {name}", container_verb(verb))),
            ),
            Intent::DeleteFile { path } | Intent::DeleteTree { path } => {
                (None, Some(format!("delete {path}")))
            }
            Intent::EditFile { path } => (None, Some(format!("write {path}"))),
            Intent::KillProcess { .. } => (Some("kill".into()), Some("signal processes".into())),
            Intent::Package { .. } => (None, Some("change packages".into())),
            _ => (None, Some("other host change".into())),
        };
        if let Some(program) = program.filter(|program| is_program_name(program)) {
            push_capped(&mut facts.programs, program);
        }
        if let Some(operation) = operation {
            push_capped(&mut facts.operations, operation);
        }
    }
    facts
}

/// A program is a bare name or an absolute path; anything else may be an argument.
fn is_program_name(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 256
        && !word.contains('=')
        && (word.starts_with('/') || !word.contains('/'))
        && word
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | '+'))
}

/// systemctl treats a name without a unit suffix as a service.
fn unit_name(unit: &str) -> String {
    if unit.contains('.') {
        unit.to_string()
    } else {
        format!("{unit}.service")
    }
}

fn push_capped(values: &mut Vec<String>, value: String) {
    if values.len() < FACT_CAP && !values.contains(&value) {
        values.push(value);
    }
}

fn service_verb(verb: ServiceVerb) -> &'static str {
    match verb {
        ServiceVerb::Restart => "restart",
        ServiceVerb::Reload => "reload",
        ServiceVerb::Stop => "stop",
        ServiceVerb::Disable | ServiceVerb::DisableNow => "disable",
        ServiceVerb::Start => "start",
    }
}

fn container_verb(verb: ContainerVerb) -> &'static str {
    match verb {
        ContainerVerb::Restart => "restart",
        ContainerVerb::Stop => "stop",
        ContainerVerb::Remove => "remove",
    }
}
