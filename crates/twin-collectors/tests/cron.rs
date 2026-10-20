use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use twin_collectors::{collect_cron, CronWarningKind};

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(path, content).expect("write");
}

fn summary(root: &Path) -> Vec<(String, usize, String, String, String)> {
    collect_cron(root)
        .entries()
        .iter()
        .map(|entry| {
            (
                entry
                    .path()
                    .strip_prefix(root)
                    .expect("under root")
                    .display()
                    .to_string(),
                entry.line(),
                entry.user().unwrap_or_default().to_string(),
                entry.schedule().to_string(),
                entry.command().to_string(),
            )
        })
        .collect()
}

#[test]
fn reads_system_cron_d_user_spools_and_run_parts() {
    let root = tempfile::tempdir().expect("root");
    write(
        root.path(),
        "etc/crontab",
        "SHELL=/bin/sh\nPGPASSWORD=SENTINEL_ENV\n# comment\n17 * * * * root cd / && run-parts --report /etc/cron.hourly\n",
    );
    write(
        root.path(),
        "etc/cron.d/backup",
        "@daily backup /usr/local/bin/backup.sh --to /srv/backup\n*/5 1-3 * jan mon-fri root /usr/bin/redis-cli ping\n",
    );
    write(
        root.path(),
        "etc/cron.d/ignored.dpkg-old",
        "@daily root /bin/true\n",
    );
    write(
        root.path(),
        "var/spool/cron/crontabs/alice",
        "0 3 * * * /home/alice/sync\n",
    );
    write(
        root.path(),
        "var/spool/cron/bob",
        "@reboot /usr/bin/notify start\n",
    );
    write(root.path(), "etc/cron.daily/logrotate", "#!/bin/sh\n");
    let entries = summary(root.path());
    let expected = [
        (
            "etc/crontab",
            4,
            "root",
            "17 * * * *",
            "cd / && run-parts --report /etc/cron.hourly",
        ),
        (
            "etc/cron.d/backup",
            1,
            "backup",
            "@daily",
            "/usr/local/bin/backup.sh --to /srv/backup",
        ),
        (
            "etc/cron.d/backup",
            2,
            "root",
            "*/5 1-3 * jan mon-fri",
            "/usr/bin/redis-cli ping",
        ),
        (
            "var/spool/cron/crontabs/alice",
            1,
            "alice",
            "0 3 * * *",
            "/home/alice/sync",
        ),
        (
            "var/spool/cron/bob",
            1,
            "bob",
            "@reboot",
            "/usr/bin/notify start",
        ),
        (
            "etc/cron.daily/logrotate",
            0,
            "root",
            "@daily",
            "/etc/cron.daily/logrotate",
        ),
    ];
    assert_eq!(entries.len(), expected.len(), "{entries:?}");
    for (path, line, user, schedule, command) in expected {
        assert!(
            entries.iter().any(|entry| entry
                == &(
                    path.into(),
                    line,
                    user.into(),
                    schedule.into(),
                    command.into()
                )),
            "missing {path}:{line} in {entries:?}"
        );
    }
    assert!(entries
        .iter()
        .all(|entry| !entry.4.contains("SENTINEL_ENV")));
    assert!(collect_cron(root.path()).is_complete());
}

#[test]
fn stdin_after_an_unescaped_percent_is_not_part_of_the_command() {
    let root = tempfile::tempdir().expect("root");
    write(
        root.path(),
        "etc/cron.d/mail",
        "@hourly root /usr/bin/date +\\%s%SENTINEL_STDIN\n",
    );
    let entries = summary(root.path());
    assert_eq!(entries[0].4, "/usr/bin/date +\\%s");
}

#[test]
fn malformed_entries_warn_without_content_and_debug_redacts_commands() {
    let root = tempfile::tempdir().expect("root");
    write(
        root.path(),
        "etc/cron.d/broken",
        "@sometimes root /bin/true\n* * * root\n1 2 3 4 5 root mysql -pSENTINEL\n",
    );
    let batch = collect_cron(root.path());
    assert_eq!(batch.entries().len(), 1);
    let malformed: Vec<_> = batch
        .warnings()
        .iter()
        .filter(|warning| warning.kind() == CronWarningKind::Malformed)
        .collect();
    assert_eq!(malformed.len(), 2);
    assert!(
        batch.is_complete(),
        "a malformed line does not hide other jobs"
    );
    let debug = format!("{:?}", batch);
    assert!(!debug.contains("SENTINEL"), "{debug}");
}

#[test]
fn unreadable_spools_and_oversized_files_are_coverage_gaps() {
    let root = tempfile::tempdir().expect("root");
    write(
        root.path(),
        "var/spool/cron/crontabs/alice",
        "0 3 * * * /bin/true\n",
    );
    let spool = root.path().join("var/spool/cron/crontabs");
    std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    write(
        root.path(),
        "etc/cron.d/huge",
        &"0 0 * * * root /bin/true\n".repeat(4_000),
    );
    let batch = collect_cron(root.path());
    std::fs::set_permissions(&spool, std::fs::Permissions::from_mode(0o755)).expect("restore");
    let kinds: Vec<_> = batch
        .warnings()
        .iter()
        .map(|warning| warning.kind())
        .collect();
    assert!(kinds.contains(&CronWarningKind::Oversized), "{kinds:?}");
    assert!(!batch.is_complete());
    // Root reads through mode 000, so only an unprivileged run can see the gap.
    if !is_root() {
        assert!(kinds.contains(&CronWarningKind::Unreadable), "{kinds:?}");
        assert!(batch.entries().is_empty());
    }
}

#[test]
fn a_missing_cron_installation_is_empty_and_complete() {
    let root = tempfile::tempdir().expect("root");
    let batch = collect_cron(root.path());
    assert!(batch.entries().is_empty());
    assert!(batch.warnings().is_empty());
    assert!(batch.is_complete());
}

fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("Uid:"))
                .and_then(|line| line.split_whitespace().nth(1).map(|uid| uid == "0"))
        })
        .unwrap_or(false)
}
