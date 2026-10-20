use std::path::{Path, PathBuf};

use super::intent::{classify_line, Intent};
use crate::model::PreflightVerdict;

const MAX_SCRIPT_BYTES: u64 = 64 * 1024;
const MAX_DEPTH: usize = 3;

/// Replaces script and make invocations with the operations inside them. Only lines
/// that resolve to host operations are kept: other script lines can hold credentials
/// and are never surfaced.
pub(crate) fn expand(intents: Vec<(String, Intent)>, cwd: Option<&Path>) -> Vec<(String, Intent)> {
    expand_at(intents, cwd, 0)
}

/// Script and make invocations stay unassessed: a caution, never a guess.
pub(crate) fn without_reading(intents: Vec<(String, Intent)>) -> Vec<(String, Intent)> {
    intents
        .into_iter()
        .map(|(segment, intent)| match intent {
            Intent::RunScript { .. } | Intent::RunMake { .. } => not_inspected(
                segment,
                "the twin daemon does not read files for clients; run twin preflight --no-daemon to inspect it",
            ),
            other => (segment, other),
        })
        .collect()
}

fn expand_at(
    intents: Vec<(String, Intent)>,
    cwd: Option<&Path>,
    depth: usize,
) -> Vec<(String, Intent)> {
    let mut out = Vec::new();
    for (segment, intent) in intents {
        match intent {
            Intent::RunScript { path } => out.extend(shell_script(&path, cwd, depth)),
            Intent::RunMake {
                directory,
                file,
                targets,
            } => out.extend(makefile(
                directory.as_deref(),
                file.as_deref(),
                &targets,
                cwd,
                depth,
            )),
            other => out.push((segment, other)),
        }
    }
    out
}

fn resolve(path: &str, cwd: Option<&Path>) -> PathBuf {
    let path = Path::new(path);
    match cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    }
}

fn not_inspected(label: String, reason: &'static str) -> (String, Intent) {
    (
        label.clone(),
        Intent::Advisory {
            operation: label,
            verdict: PreflightVerdict::Caution,
            reason,
        },
    )
}

fn read_bounded(path: &Path) -> Result<String, &'static str> {
    let metadata = std::fs::metadata(path).map_err(|_| "script not found or not readable")?;
    if !metadata.is_file() {
        return Err("not a regular file");
    }
    if metadata.len() > MAX_SCRIPT_BYTES {
        return Err("script larger than 64 KiB; contents not inspected");
    }
    let bytes = std::fs::read(path).map_err(|_| "script not readable")?;
    String::from_utf8(bytes).map_err(|_| "binary file")
}

fn shell_script(raw: &str, cwd: Option<&Path>, depth: usize) -> Vec<(String, Intent)> {
    let path = resolve(raw, cwd);
    let label = format!("run {}", path.display());
    if depth >= MAX_DEPTH {
        return vec![not_inspected(
            label,
            "scripts nested too deeply; inner script not inspected",
        )];
    }
    let text = match read_bounded(&path) {
        Ok(text) => text,
        Err("binary file") => {
            return vec![(
                label,
                Intent::Unrecognized {
                    tool: path.display().to_string(),
                },
            )]
        }
        Err(reason) => return vec![not_inspected(label, reason)],
    };
    if let Some(interpreter) = text.lines().next().and_then(|line| line.strip_prefix("#!")) {
        let is_shell = ["sh", "bash", "zsh", "dash", "ksh"].iter().any(|shell| {
            interpreter
                .split_whitespace()
                .any(|word| word.rsplit('/').next() == Some(shell))
        });
        if !is_shell {
            return vec![(
                label,
                Intent::Unrecognized {
                    tool: path.display().to_string(),
                },
            )];
        }
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let script_dir = path.parent().map(Path::to_path_buf);
    let mut out = Vec::new();
    for (number, line) in logical_lines(&text) {
        let intents = classify_line(&line);
        let intents = expand_at(intents, script_dir.as_deref().or(cwd), depth + 1);
        for (_, intent) in intents {
            if let Some(kept) = keep(&name, number, &line, intent) {
                out.push(kept);
            }
        }
    }
    out
}

/// Joins backslash continuations and skips comments and heredoc bodies, which are data
/// rather than commands.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut pending = String::new();
    let mut start = 0;
    let mut heredoc: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        if let Some(delimiter) = &heredoc {
            if line.trim() == delimiter {
                heredoc = None;
            }
            continue;
        }
        if pending.is_empty() {
            start = index + 1;
            if line.trim_start().starts_with('#') {
                continue;
            }
        }
        if let Some(stripped) = line.strip_suffix('\\') {
            pending.push_str(stripped);
            pending.push(' ');
            continue;
        }
        pending.push_str(line);
        heredoc = heredoc_delimiter(&pending);
        out.push((start, std::mem::take(&mut pending)));
    }
    if !pending.is_empty() {
        out.push((start, pending));
    }
    out
}

fn heredoc_delimiter(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("<<")?;
    if rest.starts_with('<') {
        return None;
    }
    let word = rest.trim_start_matches('-').split_whitespace().next()?;
    let delimiter = word.trim_matches(|c| c == '\'' || c == '"');
    (!delimiter.is_empty()).then(|| delimiter.to_string())
}

fn keep(name: &str, number: usize, line: &str, intent: Intent) -> Option<(String, Intent)> {
    let segment = format!("{name}:{number}: {}", line.trim());
    match intent {
        Intent::ReadOnly { .. } | Intent::Unrecognized { .. } | Intent::Remote { .. } => None,
        Intent::Advisory { .. } => Some((segment, intent)),
        operation if line.contains('$') => Some((
            segment.clone(),
            Intent::Advisory {
                operation: format!("{name}:{number}: {}", describe(&operation)),
                verdict: PreflightVerdict::Caution,
                reason: "uses shell variables; the target is only known at run time",
            },
        )),
        operation => Some((segment, operation)),
    }
}

fn describe(intent: &Intent) -> &'static str {
    match intent {
        Intent::Service { .. } => "service change",
        Intent::Container { .. } | Intent::Compose { .. } => "container change",
        Intent::DeleteFile { .. } | Intent::DeleteTree { .. } => "file deletion",
        Intent::EditFile { .. } => "file change",
        Intent::Package { .. } => "package change",
        Intent::KillProcess { .. } => "process signal",
        Intent::Unmount { .. } | Intent::WipeDevice { .. } => "storage change",
        Intent::InterfaceDown { .. } | Intent::BlockPort { .. } | Intent::Firewall { .. } => {
            "network change"
        }
        _ => "host change",
    }
}

fn makefile(
    directory: Option<&str>,
    file: Option<&str>,
    targets: &[String],
    cwd: Option<&Path>,
    depth: usize,
) -> Vec<(String, Intent)> {
    let base = directory
        .map(|dir| resolve(dir, cwd))
        .or_else(|| cwd.map(Path::to_path_buf));
    let candidates = match file {
        Some(file) => vec![resolve(file, base.as_deref())],
        None => ["GNUmakefile", "makefile", "Makefile"]
            .iter()
            .map(|name| resolve(name, base.as_deref()))
            .collect(),
    };
    let Some(path) = candidates.into_iter().find(|path| path.is_file()) else {
        return vec![not_inspected(
            "make".into(),
            "no Makefile found; recipes not inspected",
        )];
    };
    let text = match read_bounded(&path) {
        Ok(text) => text,
        Err(reason) => return vec![not_inspected(format!("make -f {}", path.display()), reason)],
    };
    let rules = parse_rules(&text);
    let goals = if targets.is_empty() {
        rules
            .first()
            .map(|rule| vec![rule.name.clone()])
            .unwrap_or_default()
    } else {
        targets.to_vec()
    };
    let mut visited = Vec::new();
    let mut out = Vec::new();
    for goal in goals {
        recipe_intents(
            &rules,
            &goal,
            depth,
            base.as_deref(),
            &mut visited,
            &mut out,
        );
    }
    out
}

struct Rule {
    name: String,
    prerequisites: Vec<String>,
    recipe: Vec<(usize, String)>,
}

fn parse_rules(text: &str) -> Vec<Rule> {
    let mut rules: Vec<Rule> = Vec::new();
    let mut current: Option<usize> = None;
    for (index, line) in text.lines().enumerate() {
        if let Some(command) = line.strip_prefix('\t') {
            if let Some(rule) = current.and_then(|i| rules.get_mut(i)) {
                let command = command.trim_start_matches(['@', '-', '+']).trim();
                if !command.is_empty() {
                    rule.recipe.push((index + 1, command.to_string()));
                }
            }
            continue;
        }
        current = None;
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.contains('=') && !trimmed.contains(':')
        {
            continue;
        }
        let Some((names, prerequisites)) = trimmed.split_once(':') else {
            continue;
        };
        if prerequisites.starts_with('=') {
            continue;
        }
        let prerequisites = prerequisites
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        for name in names.split_whitespace() {
            if name.starts_with('.') || name.contains('%') {
                continue;
            }
            rules.push(Rule {
                name: name.to_string(),
                prerequisites: prerequisites.clone(),
                recipe: Vec::new(),
            });
            current = Some(rules.len() - 1);
        }
    }
    rules
}

fn recipe_intents(
    rules: &[Rule],
    goal: &str,
    depth: usize,
    cwd: Option<&Path>,
    visited: &mut Vec<String>,
    out: &mut Vec<(String, Intent)>,
) {
    if depth >= MAX_DEPTH || visited.iter().any(|seen| seen == goal) {
        return;
    }
    visited.push(goal.to_string());
    let Some(rule) = rules.iter().find(|rule| rule.name == goal) else {
        return;
    };
    for prerequisite in &rule.prerequisites {
        recipe_intents(rules, prerequisite, depth + 1, cwd, visited, out);
    }
    let name = format!("make {goal}");
    for (number, line) in &rule.recipe {
        if line.contains("$(MAKE)") || line.contains("${MAKE}") {
            continue;
        }
        let intents = expand_at(classify_line(line), cwd, depth + 1);
        for (_, intent) in intents {
            if let Some(kept) = keep(&name, *number, line, intent) {
                out.push(kept);
            }
        }
    }
}
