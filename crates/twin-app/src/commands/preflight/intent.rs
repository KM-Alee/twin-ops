use super::shell::{split_commands, SimpleCommand};
use crate::model::PreflightVerdict;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServiceVerb {
    Restart,
    Reload,
    Stop,
    Disable,
    DisableNow,
    Start,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerVerb {
    Restart,
    Stop,
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageVerb {
    Change,
    Remove,
    SystemUpgrade,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProcessTarget {
    Pid(u32),
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Intent {
    ReadOnly {
        tool: String,
    },
    Service {
        verb: ServiceVerb,
        unit: String,
    },
    Container {
        verb: ContainerVerb,
        name: String,
    },
    DeleteFile {
        path: String,
    },
    DeleteTree {
        path: String,
    },
    EditFile {
        path: String,
    },
    Package {
        verb: PackageVerb,
        packages: Vec<String>,
    },
    KillProcess {
        target: ProcessTarget,
    },
    K8sRollout {
        namespace: String,
        deployment: String,
    },
    K8sDeletePod {
        namespace: String,
        pod: String,
    },
    K8sDestructive {
        detail: String,
    },
    BlockPort {
        port: u16,
    },
    Firewall {
        tool: String,
    },
    HostPower {
        action: String,
    },
    Remote {
        tool: String,
    },
    Unmount {
        target: String,
    },
    InterfaceDown {
        interface: String,
    },
    WipeDevice {
        device: String,
    },
    RunScript {
        path: String,
    },
    RunMake {
        directory: Option<String>,
        file: Option<String>,
        targets: Vec<String>,
    },
    Compose {
        verb: ContainerVerb,
        project: Option<String>,
        file: Option<String>,
        services: Vec<String>,
    },
    /// A change twin recognizes but cannot assess on the graph; the verdict and reason
    /// are fixed by what the command is known to do.
    Advisory {
        operation: String,
        verdict: PreflightVerdict,
        reason: &'static str,
    },
    Unrecognized {
        tool: String,
    },
}

const MAX_NESTING: usize = 3;

pub(crate) fn classify_line(line: &str) -> Vec<(String, Intent)> {
    classify_nested(line, 0)
}

fn classify_nested(line: &str, depth: usize) -> Vec<(String, Intent)> {
    let mut out = Vec::new();
    for command in split_commands(line) {
        let segment = display(&command);
        for target in &command.redirect_targets {
            if target.starts_with("/proc/sys/") {
                out.push((
                    segment.clone(),
                    advisory(
                        format!("write kernel setting {target}"),
                        PreflightVerdict::Caution,
                        "changes a live kernel setting host-wide",
                    ),
                ));
            } else if !target.starts_with("/dev/") && !target.starts_with('&') {
                out.push((
                    segment.clone(),
                    Intent::EditFile {
                        path: target.clone(),
                    },
                ));
            }
        }
        let words = strip_wrappers(&command.words);
        let Some(tool) = words.first() else {
            continue;
        };
        if matches!(base(tool), "bash" | "sh" | "zsh" | "dash" | "fish") {
            if let Some(index) = words.iter().position(|word| word == "-c") {
                if let Some(script) = words.get(index + 1) {
                    if depth < MAX_NESTING {
                        out.extend(classify_nested(script, depth + 1));
                    }
                    continue;
                }
            }
        }
        for intent in classify_words(&words) {
            out.push((segment.clone(), intent));
        }
    }
    out
}

fn display(command: &SimpleCommand) -> String {
    let mut parts = command.words.clone();
    for target in &command.redirect_targets {
        parts.push(format!("> {target}"));
    }
    parts.join(" ")
}

fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn strip_wrappers(words: &[String]) -> Vec<String> {
    let mut index = 0;
    while index < words.len() {
        let word = words[index].as_str();
        if is_env_assignment(word) {
            index += 1;
            continue;
        }
        match base(word) {
            "sudo" | "doas" | "run0" => {
                index += 1;
                while index < words.len() && words[index].starts_with('-') {
                    let takes_value = matches!(words[index].as_str(), "-u" | "-g" | "-U" | "-C");
                    index += if takes_value { 2 } else { 1 };
                }
            }
            "env" | "nohup" | "nice" | "ionice" | "time" | "command" | "exec" | "stdbuf"
            | "chrt" => {
                index += 1;
                while index < words.len()
                    && (words[index].starts_with('-') || is_env_assignment(&words[index]))
                {
                    index += 1;
                }
            }
            "timeout" => {
                index += 1;
                while index < words.len() && words[index].starts_with('-') {
                    index += 1;
                }
                index += 1;
            }
            _ => break,
        }
    }
    words[index.min(words.len())..].to_vec()
}

fn is_env_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

fn positional(words: &[String]) -> Vec<&str> {
    words
        .iter()
        .map(String::as_str)
        .filter(|word| !word.starts_with('-'))
        .collect()
}

fn classify_words(words: &[String]) -> Vec<Intent> {
    let Some(first) = words.first() else {
        return Vec::new();
    };
    let tool = base(first).to_string();
    let rest = &words[1..];
    match tool.as_str() {
        "systemctl" => systemctl(rest),
        "service" => service(rest),
        "docker" | "podman" | "nerdctl" => container_cli(&tool, rest),
        "rm" | "unlink" | "shred" => delete(rest),
        "truncate" => file_args(rest, &["-s", "--size", "-r", "--reference"]),
        "tee" => file_args(rest, &[]),
        "sed" | "perl" if rest.iter().any(|w| w == "-i" || w.starts_with("-i")) => {
            sed_targets(rest)
        }
        "mv" | "cp" | "install" | "ln" => last_arg_edit(rest),
        "chmod" | "chown" | "chgrp" => {
            let args = positional(rest);
            args.iter()
                .skip(1)
                .map(|path| Intent::EditFile {
                    path: path.to_string(),
                })
                .collect()
        }
        "apt" | "apt-get" | "aptitude" => apt(rest),
        "dnf" | "yum" | "microdnf" => dnf(rest),
        "zypper" => zypper(rest),
        "apk" => apk(rest),
        "pacman" | "yay" | "paru" => pacman(rest),
        "dpkg" | "rpm" => dpkg_rpm(rest),
        "kill" => kill(rest),
        "umount" => positional(rest)
            .into_iter()
            .map(|target| Intent::Unmount {
                target: target.to_string(),
            })
            .collect(),
        "mount" => mount(rest),
        "ip" => ip(rest),
        "ifdown" => positional(rest)
            .first()
            .map(|interface| {
                vec![Intent::InterfaceDown {
                    interface: interface.to_string(),
                }]
            })
            .unwrap_or_default(),
        "nmcli" => nmcli(rest),
        "dd" => dd(rest),
        "wipefs" | "sgdisk" | "sfdisk" | "blkdiscard" | "mkswap" => wipe(&tool, rest),
        "parted" if !rest.iter().any(|w| w == "-l" || w == "print") => wipe(&tool, rest),
        tool_name if tool_name.starts_with("mkfs") => wipe(tool_name, rest),
        "cryptsetup" if rest.iter().any(|w| w == "luksFormat" || w == "erase") => wipe(&tool, rest),
        "swapoff" => vec![advisory(
            "swapoff".into(),
            PreflightVerdict::Caution,
            "removes swap; memory pressure may trigger OOM kills",
        )],
        "sysctl" if rest.iter().any(|w| w == "-w" || w.contains('=')) => vec![advisory(
            format!("sysctl {}", positional(rest).join(" ")),
            PreflightVerdict::Caution,
            "changes a live kernel setting host-wide",
        )],
        "crontab" if rest.iter().any(|w| w == "-r") => vec![advisory(
            "crontab -r".into(),
            PreflightVerdict::Caution,
            "removes every scheduled job of this user",
        )],
        "crontab" if rest.iter().any(|w| w == "-l") => vec![Intent::ReadOnly { tool }],
        "crontab" => vec![advisory(
            "replace crontab".into(),
            PreflightVerdict::Caution,
            "replaces this user's scheduled jobs",
        )],
        "userdel" | "groupdel" => vec![advisory(
            format!("{tool} {}", positional(rest).join(" ")),
            PreflightVerdict::Caution,
            "services or files owned by this account may stop working",
        )],
        "usermod" | "passwd" | "chage"
            if rest.iter().any(|w| {
                matches!(
                    w.as_str(),
                    "-L" | "-l" | "-d" | "-e" | "--lock" | "--expiredate"
                )
            }) =>
        {
            vec![advisory(
                format!("{tool} {}", rest.join(" ")),
                PreflightVerdict::Caution,
                "locks or expires an account; services or logins using it may fail",
            )]
        }
        "docker-compose" => compose(None, rest),
        "bash" | "sh" | "zsh" | "dash" | "ksh" | "source" | "." => positional(rest)
            .first()
            .map(|script| {
                vec![Intent::RunScript {
                    path: script.to_string(),
                }]
            })
            .unwrap_or_else(|| vec![Intent::Unrecognized { tool: tool.clone() }]),
        "make" | "gmake" => make(rest),
        "pkill" | "killall" => positional(rest)
            .first()
            .map(|name| {
                vec![Intent::KillProcess {
                    target: ProcessTarget::Name(name.to_string()),
                }]
            })
            .unwrap_or_default(),
        "kubectl" | "oc" => kubectl(rest),
        "ufw" => ufw(rest),
        "iptables" | "ip6tables" | "nft" | "firewall-cmd" => firewall(&tool, rest),
        "reboot" | "poweroff" | "halt" | "shutdown" | "kexec" => vec![Intent::HostPower {
            action: tool.clone(),
        }],
        "ssh" | "scp" | "rsync" | "ansible" | "ansible-playbook" => {
            vec![Intent::Remote { tool }]
        }
        "nginx" | "apachectl" | "apache2ctl" | "httpd" => web_server_signal(&tool, rest),
        "journalctl" | "ss" | "netstat" | "lsof" | "ps" | "top" | "htop" | "cat" | "less"
        | "head" | "tail" | "grep" | "rg" | "ls" | "df" | "du" | "free" | "uptime" | "curl"
        | "wget" | "dig" | "host" | "twin" | "ifup" | "lsblk" | "blkid" | "findmnt" => {
            vec![Intent::ReadOnly { tool }]
        }
        _ if first.contains('/') => vec![Intent::RunScript {
            path: first.clone(),
        }],
        _ => vec![Intent::Unrecognized { tool }],
    }
}

fn make(rest: &[String]) -> Vec<Intent> {
    let mut directory = None;
    let mut file = None;
    let mut targets = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        let word = rest[index].as_str();
        match word {
            "-C" | "--directory" => {
                directory = rest.get(index + 1).cloned();
                index += 2;
                continue;
            }
            "-f" | "--file" | "--makefile" => {
                file = rest.get(index + 1).cloned();
                index += 2;
                continue;
            }
            "-j" | "--jobs" | "-l" | "--load-average"
                if rest
                    .get(index + 1)
                    .is_some_and(|next| next.parse::<f64>().is_ok()) =>
            {
                index += 2;
                continue;
            }
            _ => {}
        }
        if !word.starts_with('-') && !word.contains('=') {
            targets.push(word.to_string());
        }
        index += 1;
    }
    vec![Intent::RunMake {
        directory,
        file,
        targets,
    }]
}

fn systemctl(rest: &[String]) -> Vec<Intent> {
    let has_now = rest.iter().any(|w| w == "--now");
    let args: Vec<&str> = positional(rest);
    let Some((verb, units)) = args.split_first() else {
        return vec![Intent::ReadOnly {
            tool: "systemctl".into(),
        }];
    };
    let verb = match *verb {
        "restart"
        | "try-restart"
        | "reload-or-restart"
        | "try-reload-or-restart"
        | "condrestart" => ServiceVerb::Restart,
        "reload" => ServiceVerb::Reload,
        "stop" | "kill" | "freeze" => ServiceVerb::Stop,
        "disable" | "mask" if has_now => ServiceVerb::DisableNow,
        "disable" | "mask" => ServiceVerb::Disable,
        "start" | "enable" | "unmask" | "thaw" => ServiceVerb::Start,
        "set-default" => {
            return vec![advisory(
                "systemctl set-default".into(),
                PreflightVerdict::Caution,
                "changes which target the host boots into",
            )]
        }
        "reboot" | "poweroff" | "halt" | "kexec" | "isolate" | "rescue" | "emergency"
        | "soft-reboot" => {
            return vec![Intent::HostPower {
                action: format!("systemctl {verb}"),
            }]
        }
        _ => {
            return vec![Intent::ReadOnly {
                tool: "systemctl".into(),
            }]
        }
    };
    units
        .iter()
        .map(|unit| Intent::Service {
            verb,
            unit: unit.to_string(),
        })
        .collect()
}

fn service(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.as_slice() {
        [unit, verb, ..] => {
            let verb = match *verb {
                "restart" | "force-reload" | "condrestart" | "try-restart" => ServiceVerb::Restart,
                "reload" => ServiceVerb::Reload,
                "stop" => ServiceVerb::Stop,
                "start" => ServiceVerb::Start,
                _ => {
                    return vec![Intent::ReadOnly {
                        tool: "service".into(),
                    }]
                }
            };
            vec![Intent::Service {
                verb,
                unit: unit.to_string(),
            }]
        }
        _ => vec![Intent::ReadOnly {
            tool: "service".into(),
        }],
    }
}

fn container_cli(tool: &str, rest: &[String]) -> Vec<Intent> {
    let mut args: Vec<&str> = positional(rest);
    if args.first() == Some(&"container") {
        args.remove(0);
    }
    if args.first() == Some(&"compose") {
        let start = rest
            .iter()
            .position(|w| w == "compose")
            .map_or(0, |i| i + 1);
        return compose(Some(tool), &rest[start..]);
    }
    let Some((verb, names)) = args.split_first() else {
        return vec![Intent::ReadOnly { tool: tool.into() }];
    };
    let verb = match *verb {
        "restart" => ContainerVerb::Restart,
        "stop" | "kill" | "pause" => ContainerVerb::Stop,
        "rm" | "remove" => ContainerVerb::Remove,
        "system" | "volume" | "network" | "swarm" | "service" | "stack" => {
            let sub = names.first().copied().unwrap_or_default();
            if matches!(
                sub,
                "down" | "stop" | "restart" | "rm" | "prune" | "kill" | "remove" | "scale"
            ) {
                return vec![Intent::Unrecognized {
                    tool: format!("{tool} {verb} {sub}"),
                }];
            }
            return vec![Intent::ReadOnly { tool: tool.into() }];
        }
        _ => return vec![Intent::ReadOnly { tool: tool.into() }],
    };
    names
        .iter()
        .map(|name| Intent::Container {
            verb,
            name: name.to_string(),
        })
        .collect()
}

fn delete(rest: &[String]) -> Vec<Intent> {
    let recursive = rest.iter().any(|word| {
        word == "--recursive"
            || (word.starts_with('-')
                && !word.starts_with("--")
                && (word.contains('r') || word.contains('R')))
    });
    positional(rest)
        .into_iter()
        .map(|path| {
            if recursive {
                Intent::DeleteTree { path: path.into() }
            } else {
                Intent::DeleteFile { path: path.into() }
            }
        })
        .collect()
}

fn file_args(rest: &[String], value_flags: &[&str]) -> Vec<Intent> {
    let mut out = Vec::new();
    let mut skip = false;
    for word in rest {
        if skip {
            skip = false;
            continue;
        }
        if value_flags.contains(&word.as_str()) {
            skip = true;
            continue;
        }
        if word.starts_with('-') {
            continue;
        }
        out.push(Intent::EditFile { path: word.clone() });
    }
    out
}

fn sed_targets(rest: &[String]) -> Vec<Intent> {
    let mut args = Vec::new();
    let mut has_script_flag = false;
    let mut skip = false;
    for word in rest {
        if skip {
            skip = false;
            continue;
        }
        if word == "-e" || word == "-f" {
            has_script_flag = true;
            skip = true;
            continue;
        }
        if word.starts_with('-') {
            continue;
        }
        args.push(word.clone());
    }
    let files = if has_script_flag {
        &args[..]
    } else {
        args.get(1..).unwrap_or_default()
    };
    files
        .iter()
        .map(|path| Intent::EditFile { path: path.clone() })
        .collect()
}

fn last_arg_edit(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.as_slice() {
        [.., _, last] => vec![Intent::EditFile {
            path: last.to_string(),
        }],
        _ => Vec::new(),
    }
}

fn package_intent(verb: &str, packages: &[&str], tool: &str) -> Vec<Intent> {
    let packages: Vec<String> = packages.iter().map(|p| p.to_string()).collect();
    let verb = match verb {
        "remove" | "purge" | "erase" | "autoremove" | "del" | "rm" => PackageVerb::Remove,
        "upgrade" | "full-upgrade" | "dist-upgrade" | "update" | "up" | "dup"
            if packages.is_empty() =>
        {
            PackageVerb::SystemUpgrade
        }
        "install" | "reinstall" | "upgrade" | "update" | "in" | "add" | "up" | "full-upgrade"
        | "downgrade" => PackageVerb::Change,
        _ => return vec![Intent::ReadOnly { tool: tool.into() }],
    };
    if verb != PackageVerb::SystemUpgrade && packages.is_empty() {
        return vec![Intent::ReadOnly { tool: tool.into() }];
    }
    vec![Intent::Package { verb, packages }]
}

fn apt(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.split_first() {
        Some((&"update", _)) => vec![Intent::ReadOnly { tool: "apt".into() }],
        Some((verb, packages)) => package_intent(verb, packages, "apt"),
        None => vec![Intent::ReadOnly { tool: "apt".into() }],
    }
}

fn dnf(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.split_first() {
        Some((verb, packages)) => package_intent(verb, packages, "dnf"),
        None => vec![Intent::ReadOnly { tool: "dnf".into() }],
    }
}

fn zypper(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.split_first() {
        Some((verb, packages)) => package_intent(verb, packages, "zypper"),
        None => vec![Intent::ReadOnly {
            tool: "zypper".into(),
        }],
    }
}

fn apk(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.split_first() {
        Some((&"update", _)) => vec![Intent::ReadOnly { tool: "apk".into() }],
        Some((verb, packages)) => package_intent(verb, packages, "apk"),
        None => vec![Intent::ReadOnly { tool: "apk".into() }],
    }
}

fn pacman(rest: &[String]) -> Vec<Intent> {
    let operation = rest
        .iter()
        .find(|word| word.starts_with('-') && !word.starts_with("--"))
        .map(String::as_str)
        .unwrap_or_default();
    let packages = positional(rest);
    if operation.starts_with("-R") {
        return package_intent("remove", &packages, "pacman");
    }
    if operation.starts_with("-S") || operation.starts_with("-U") {
        let query_only = operation.contains('s')
            || operation.contains('i')
            || operation.contains('w')
            || (operation.contains('c') && !operation.contains('u'));
        if query_only {
            return vec![Intent::ReadOnly {
                tool: "pacman".into(),
            }];
        }
        if packages.is_empty() && operation.contains('u') {
            return package_intent("upgrade", &[], "pacman");
        }
        return package_intent("install", &packages, "pacman");
    }
    vec![Intent::ReadOnly {
        tool: "pacman".into(),
    }]
}

fn dpkg_rpm(rest: &[String]) -> Vec<Intent> {
    let removing = rest.iter().any(|w| {
        matches!(
            w.as_str(),
            "-r" | "-P" | "--remove" | "--purge" | "-e" | "--erase"
        )
    });
    let installing = rest.iter().any(|w| {
        matches!(
            w.as_str(),
            "-i" | "--install" | "-U" | "--upgrade" | "-ivh" | "-Uvh"
        )
    });
    let packages = positional(rest);
    if removing {
        return package_intent("remove", &packages, "dpkg");
    }
    if installing {
        return package_intent("install", &packages, "dpkg");
    }
    vec![Intent::ReadOnly {
        tool: "dpkg".into(),
    }]
}

fn kill(rest: &[String]) -> Vec<Intent> {
    let signal = rest
        .iter()
        .position(|w| w == "-s" || w == "-n")
        .and_then(|index| rest.get(index + 1))
        .map(|w| w.trim_start_matches("SIG").to_ascii_uppercase())
        .or_else(|| {
            rest.iter()
                .find(|w| w.starts_with('-') && w.len() > 1)
                .map(|w| w[1..].trim_start_matches("SIG").to_ascii_uppercase())
        });
    if rest.iter().any(|w| w == "-l" || w == "-L")
        || matches!(signal.as_deref(), Some("0" | "CONT" | "18"))
    {
        return vec![Intent::ReadOnly {
            tool: "kill".into(),
        }];
    }
    if matches!(
        signal.as_deref(),
        Some("HUP" | "1" | "USR1" | "USR2" | "10" | "12")
    ) {
        return vec![advisory(
            format!("kill {}", rest.join(" ")),
            PreflightVerdict::Caution,
            "daemons that handle this signal reload or reopen logs; processes that don't handle it terminate",
        )];
    }
    let mut out = Vec::new();
    let mut skip = false;
    for word in rest {
        if skip {
            skip = false;
            continue;
        }
        if word == "-s" || word == "-n" {
            skip = true;
            continue;
        }
        if word.starts_with('-') {
            continue;
        }
        if let Ok(pid) = word.parse::<u32>() {
            out.push(Intent::KillProcess {
                target: ProcessTarget::Pid(pid),
            });
        }
    }
    out
}

fn kubectl(rest: &[String]) -> Vec<Intent> {
    let mut namespace = "default".to_string();
    let mut args = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        let word = rest[index].as_str();
        if word == "-n" || word == "--namespace" {
            if let Some(value) = rest.get(index + 1) {
                namespace = value.clone();
            }
            index += 2;
            continue;
        }
        if let Some(value) = word.strip_prefix("--namespace=") {
            namespace = value.to_string();
        } else if !word.starts_with('-') {
            args.push(word.to_string());
        }
        index += 1;
    }
    let read_only = vec![Intent::ReadOnly {
        tool: "kubectl".into(),
    }];
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["rollout", "restart", target, ..] => resource_name(target, "deployment")
            .map(|deployment| {
                vec![Intent::K8sRollout {
                    namespace: namespace.clone(),
                    deployment,
                }]
            })
            .unwrap_or_else(|| {
                vec![Intent::K8sDestructive {
                    detail: format!("rollout restart {target}"),
                }]
            }),
        ["delete", kind, names @ ..] if matches!(*kind, "pod" | "pods" | "po") => names
            .iter()
            .map(|pod| Intent::K8sDeletePod {
                namespace: namespace.clone(),
                pod: pod.to_string(),
            })
            .collect(),
        ["delete", target] if target.starts_with("pod/") => vec![Intent::K8sDeletePod {
            namespace,
            pod: target.trim_start_matches("pod/").to_string(),
        }],
        ["delete", rest @ ..] => vec![Intent::K8sDestructive {
            detail: format!("delete {}", rest.join(" ")),
        }],
        ["scale", ..]
        | ["drain", ..]
        | ["cordon", ..]
        | ["apply", ..]
        | ["patch", ..]
        | ["edit", ..]
        | ["replace", ..]
        | ["set", ..]
        | ["rollout", "undo", ..]
        | ["create", ..]
        | ["label", ..]
        | ["annotate", ..]
        | ["taint", ..] => {
            vec![Intent::K8sDestructive {
                detail: args.join(" "),
            }]
        }
        _ => read_only,
    }
}

fn resource_name(target: &str, kind: &str) -> Option<String> {
    let (prefix, name) = target.split_once('/')?;
    let matches_kind = match kind {
        "deployment" => matches!(prefix, "deployment" | "deployments" | "deploy"),
        _ => false,
    };
    matches_kind.then(|| name.to_string())
}

fn ufw(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.as_slice() {
        ["deny" | "reject", spec, ..] => match port_spec(spec) {
            Some(port) => vec![Intent::BlockPort { port }],
            None => vec![Intent::Firewall { tool: "ufw".into() }],
        },
        ["status", ..] | ["show", ..] | [] => vec![Intent::ReadOnly { tool: "ufw".into() }],
        _ => vec![Intent::Firewall { tool: "ufw".into() }],
    }
}

fn firewall(tool: &str, rest: &[String]) -> Vec<Intent> {
    let listing = rest.iter().any(|w| {
        matches!(
            w.as_str(),
            "-L" | "-S" | "--list" | "list" | "--list-all" | "--state" | "--list-rules"
        )
    }) || rest.first().is_some_and(|w| w == "list");
    if listing {
        return vec![Intent::ReadOnly { tool: tool.into() }];
    }
    let blocks = rest
        .iter()
        .any(|w| matches!(w.as_str(), "DROP" | "REJECT" | "drop" | "reject"));
    let port = rest
        .iter()
        .position(|w| w == "--dport" || w == "dport")
        .and_then(|index| rest.get(index + 1))
        .and_then(|value| value.parse::<u16>().ok());
    match (blocks, port) {
        (true, Some(port)) => vec![Intent::BlockPort { port }],
        _ => vec![Intent::Firewall { tool: tool.into() }],
    }
}

fn advisory(operation: String, verdict: PreflightVerdict, reason: &'static str) -> Intent {
    Intent::Advisory {
        operation,
        verdict,
        reason,
    }
}

fn mount(rest: &[String]) -> Vec<Intent> {
    let options = rest
        .iter()
        .position(|w| w == "-o")
        .and_then(|index| rest.get(index + 1))
        .map(String::as_str)
        .unwrap_or_default();
    let remount_read_only =
        options.split(',').any(|o| o == "remount") && options.split(',').any(|o| o == "ro");
    match (remount_read_only, positional(rest).last()) {
        (true, Some(target)) => vec![Intent::Unmount {
            target: target.to_string(),
        }],
        _ => vec![Intent::ReadOnly {
            tool: "mount".into(),
        }],
    }
}

fn ip(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    let interface_after = |key: &str| {
        args.iter()
            .position(|w| *w == key)
            .and_then(|index| args.get(index + 1))
            .map(|w| w.to_string())
    };
    let object = args.first().copied().unwrap_or_default();
    let is_link = matches!(object, "link" | "l");
    let is_address = matches!(object, "address" | "addr" | "a");
    let is_route = matches!(object, "route" | "r" | "ro");
    let action = args.get(1).copied().unwrap_or_default();
    if is_link && action == "set" && args.contains(&"down") {
        let interface = interface_after("dev").or_else(|| args.get(2).map(|w| w.to_string()));
        if let Some(interface) = interface {
            return vec![Intent::InterfaceDown { interface }];
        }
    }
    if is_link && matches!(action, "delete" | "del") {
        if let Some(interface) =
            interface_after("dev").or_else(|| args.get(2).map(|w| w.to_string()))
        {
            return vec![Intent::InterfaceDown { interface }];
        }
    }
    if is_address && matches!(action, "flush" | "del" | "delete") {
        if let Some(interface) = interface_after("dev") {
            return vec![Intent::InterfaceDown { interface }];
        }
    }
    if is_route && matches!(action, "del" | "delete" | "flush" | "replace" | "change") {
        let touches_default = args
            .iter()
            .any(|w| matches!(*w, "default" | "0.0.0.0/0" | "::/0"));
        if touches_default || action == "flush" {
            return vec![advisory(
                format!("ip {}", args.join(" ")),
                PreflightVerdict::Stop,
                "changes the default route; remote sessions and outbound traffic can drop",
            )];
        }
        return vec![advisory(
            format!("ip {}", args.join(" ")),
            PreflightVerdict::Caution,
            "changes routing; traffic to the affected networks can drop",
        )];
    }
    vec![Intent::ReadOnly { tool: "ip".into() }]
}

fn nmcli(rest: &[String]) -> Vec<Intent> {
    let args = positional(rest);
    match args.as_slice() {
        ["device" | "dev" | "d", "disconnect" | "down" | "delete", interface, ..] => {
            vec![Intent::InterfaceDown {
                interface: interface.to_string(),
            }]
        }
        ["connection" | "con" | "c", "down" | "delete" | "del", name, ..] => vec![advisory(
            format!("nmcli connection down {name}"),
            PreflightVerdict::Caution,
            "deactivates a NetworkManager connection; its interface loses connectivity",
        )],
        ["networking" | "n", "off", ..] | ["radio" | "r", _, "off", ..] => vec![advisory(
            format!("nmcli {}", args.join(" ")),
            PreflightVerdict::Stop,
            "turns networking off; remote sessions drop",
        )],
        _ => vec![Intent::ReadOnly {
            tool: "nmcli".into(),
        }],
    }
}

fn dd(rest: &[String]) -> Vec<Intent> {
    rest.iter()
        .filter_map(|w| w.strip_prefix("of="))
        .map(|target| {
            if target.starts_with("/dev/") && target != "/dev/null" {
                Intent::WipeDevice {
                    device: target.to_string(),
                }
            } else {
                Intent::EditFile {
                    path: target.to_string(),
                }
            }
        })
        .collect()
}

fn wipe(tool: &str, rest: &[String]) -> Vec<Intent> {
    let devices = positional(rest)
        .into_iter()
        .filter(|w| w.starts_with("/dev/"))
        .map(|device| Intent::WipeDevice {
            device: device.to_string(),
        })
        .collect::<Vec<_>>();
    if devices.is_empty() {
        return vec![advisory(
            tool.to_string(),
            PreflightVerdict::Caution,
            "disk tool without a recognizable device argument",
        )];
    }
    devices
}

fn compose(tool: Option<&str>, rest: &[String]) -> Vec<Intent> {
    let tool = tool.map_or("docker-compose".to_string(), |t| format!("{t} compose"));
    let mut project = None;
    let mut file = None;
    let mut words = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        let word = rest[index].as_str();
        match word {
            "-p" | "--project-name" => {
                project = rest.get(index + 1).cloned();
                index += 2;
                continue;
            }
            "-f" | "--file" => {
                file = rest.get(index + 1).cloned();
                index += 2;
                continue;
            }
            _ => {}
        }
        if let Some(value) = word.strip_prefix("--project-name=") {
            project = Some(value.to_string());
        } else if let Some(value) = word.strip_prefix("--file=") {
            file = Some(value.to_string());
        } else if !word.starts_with('-') {
            words.push(word.to_string());
        }
        index += 1;
    }
    let Some((verb, services)) = words.split_first() else {
        return vec![Intent::ReadOnly { tool }];
    };
    let verb = match verb.as_str() {
        "restart" => ContainerVerb::Restart,
        "stop" | "kill" | "pause" => ContainerVerb::Stop,
        "down" | "rm" => ContainerVerb::Remove,
        _ => return vec![Intent::ReadOnly { tool }],
    };
    vec![Intent::Compose {
        verb,
        project,
        file,
        services: services.to_vec(),
    }]
}

fn port_spec(spec: &str) -> Option<u16> {
    spec.split('/').next()?.parse::<u16>().ok()
}

fn web_server_signal(tool: &str, rest: &[String]) -> Vec<Intent> {
    let signal = rest
        .iter()
        .position(|w| w == "-s" || w == "-k")
        .and_then(|index| rest.get(index + 1))
        .map(String::as_str);
    let unit = match tool {
        "nginx" => "nginx",
        _ => "apache2",
    };
    match signal {
        Some("stop" | "quit") => vec![Intent::Service {
            verb: ServiceVerb::Stop,
            unit: unit.into(),
        }],
        Some("reload" | "graceful") => vec![Intent::Service {
            verb: ServiceVerb::Reload,
            unit: unit.into(),
        }],
        Some("restart") => vec![Intent::Service {
            verb: ServiceVerb::Restart,
            unit: unit.into(),
        }],
        _ => vec![Intent::ReadOnly { tool: tool.into() }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intents(line: &str) -> Vec<Intent> {
        classify_line(line).into_iter().map(|(_, i)| i).collect()
    }

    #[test]
    fn quoted_operators_and_descriptor_redirects_are_not_operations() {
        assert_eq!(
            intents("git commit -m 'systemctl restart nginx; rm -rf /' 2>&1 >/dev/null"),
            vec![Intent::Unrecognized { tool: "git".into() }]
        );
        assert_eq!(
            intents("make &> build.log"),
            vec![
                Intent::EditFile {
                    path: "build.log".into()
                },
                Intent::RunMake {
                    directory: None,
                    file: None,
                    targets: vec![]
                }
            ]
        );
    }

    #[test]
    fn package_managers_distinguish_targets_from_system_upgrades() {
        assert_eq!(
            intents("sudo apt-get install -y nginx"),
            vec![Intent::Package {
                verb: PackageVerb::Change,
                packages: vec!["nginx".into()]
            }]
        );
        assert_eq!(
            intents("pacman -Syu"),
            vec![Intent::Package {
                verb: PackageVerb::SystemUpgrade,
                packages: vec![]
            }]
        );
        assert_eq!(
            intents("pacman -Ss openssl"),
            vec![Intent::ReadOnly {
                tool: "pacman".into()
            }]
        );
        assert_eq!(
            intents("dnf remove openssl-libs"),
            vec![Intent::Package {
                verb: PackageVerb::Remove,
                packages: vec!["openssl-libs".into()]
            }]
        );
    }

    #[test]
    fn kubectl_firewall_and_sed_targets_are_extracted() {
        assert_eq!(
            intents("kubectl -n prod rollout restart deploy/api"),
            vec![Intent::K8sRollout {
                namespace: "prod".into(),
                deployment: "api".into()
            }]
        );
        assert_eq!(
            intents("ufw deny 5432/tcp"),
            vec![Intent::BlockPort { port: 5432 }]
        );
        assert_eq!(
            intents("iptables -A INPUT -p tcp --dport 22 -j DROP"),
            vec![Intent::BlockPort { port: 22 }]
        );
        assert_eq!(
            intents("sed -i.bak 's/a/b/' /etc/nginx/nginx.conf"),
            vec![Intent::EditFile {
                path: "/etc/nginx/nginx.conf".into()
            }]
        );
        assert_eq!(
            intents("docker compose down"),
            vec![Intent::Compose {
                verb: ContainerVerb::Remove,
                project: None,
                file: None,
                services: vec![],
            }]
        );
    }

    #[test]
    fn nested_shell_scripts_are_parsed_with_bounded_depth() {
        assert_eq!(
            intents("bash -c \"sh -c 'systemctl stop api'\""),
            vec![Intent::Service {
                verb: ServiceVerb::Stop,
                unit: "api".into()
            }]
        );
    }

    #[test]
    fn network_disk_and_mount_commands_are_operations_not_reads() {
        assert_eq!(
            intents("sudo ip link set dev eth0 down"),
            vec![Intent::InterfaceDown {
                interface: "eth0".into()
            }]
        );
        assert_eq!(
            intents("ip addr flush dev eth1"),
            vec![Intent::InterfaceDown {
                interface: "eth1".into()
            }]
        );
        assert_eq!(
            intents("ip -br addr show"),
            vec![Intent::ReadOnly { tool: "ip".into() }]
        );
        assert!(matches!(
            intents("ip route del default via 10.0.0.1").as_slice(),
            [Intent::Advisory {
                verdict: PreflightVerdict::Stop,
                ..
            }]
        ));
        assert_eq!(
            intents("nmcli dev disconnect wlan0"),
            vec![Intent::InterfaceDown {
                interface: "wlan0".into()
            }]
        );
        assert_eq!(
            intents("umount /var/lib/postgresql"),
            vec![Intent::Unmount {
                target: "/var/lib/postgresql".into()
            }]
        );
        assert_eq!(
            intents("mount -o remount,ro /srv"),
            vec![Intent::Unmount {
                target: "/srv".into()
            }]
        );
        assert_eq!(
            intents("mkfs.ext4 -F /dev/sdb1"),
            vec![Intent::WipeDevice {
                device: "/dev/sdb1".into()
            }]
        );
        assert_eq!(
            intents("dd if=image.iso of=/dev/sdc bs=4M"),
            vec![Intent::WipeDevice {
                device: "/dev/sdc".into()
            }]
        );
        assert_eq!(
            intents("dd if=/dev/zero of=disk.img"),
            vec![Intent::EditFile {
                path: "disk.img".into()
            }]
        );
        assert_eq!(
            intents("parted -l"),
            vec![Intent::Unrecognized {
                tool: "parted".into()
            }]
        );
    }

    #[test]
    fn kernel_account_and_cron_changes_are_advisories() {
        for line in [
            "sysctl -w net.ipv4.ip_forward=1",
            "echo 1 > /proc/sys/net/ipv4/ip_forward",
            "swapoff -a",
            "crontab -r",
            "userdel deploy",
            "usermod -L app",
            "systemctl set-default multi-user.target",
            "kill -HUP 42",
        ] {
            assert!(
                intents(line)
                    .iter()
                    .any(|intent| matches!(intent, Intent::Advisory { .. })),
                "{line}: {:?}",
                intents(line)
            );
        }
        assert_eq!(
            intents("crontab -l"),
            vec![Intent::ReadOnly {
                tool: "crontab".into()
            }]
        );
        assert_eq!(
            intents("kill -0 42"),
            vec![Intent::ReadOnly {
                tool: "kill".into()
            }]
        );
        assert_eq!(
            intents("kill -s CONT 42"),
            vec![Intent::ReadOnly {
                tool: "kill".into()
            }]
        );
        assert_eq!(
            intents("kill -STOP 42"),
            vec![Intent::KillProcess {
                target: ProcessTarget::Pid(42)
            }]
        );
    }

    #[test]
    fn compose_commands_carry_project_file_and_services() {
        assert_eq!(
            intents("docker compose -f /srv/app/compose.yml restart web worker"),
            vec![Intent::Compose {
                verb: ContainerVerb::Restart,
                project: None,
                file: Some("/srv/app/compose.yml".into()),
                services: vec!["web".into(), "worker".into()],
            }]
        );
        assert_eq!(
            intents("docker-compose --project-name=shop down"),
            vec![Intent::Compose {
                verb: ContainerVerb::Remove,
                project: Some("shop".into()),
                file: None,
                services: vec![],
            }]
        );
        assert_eq!(
            intents("docker compose ps"),
            vec![Intent::ReadOnly {
                tool: "docker compose".into()
            }]
        );
    }
}
