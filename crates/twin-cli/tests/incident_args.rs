use clap::Parser;
use twin_cli::cli::{
    args::{Cli, Command},
    what_changed::what_changed_request,
};
use twin_core::ChangeCategory;

#[test]
fn incident_flags_compile_to_typed_request() {
    let cli = Cli::try_parse_from([
        "twin",
        "what-changed",
        "--since",
        "30m",
        "--target",
        "nginx",
        "--category",
        "config_changed",
        "--category",
        "service_restarted",
        "--limit",
        "10",
        "--max-depth",
        "3",
    ])
    .expect("args");
    let Command::WhatChanged(args) = cli.command else {
        panic!("wrong command");
    };
    let request = what_changed_request(&args);
    assert_eq!(request.target_query.as_deref(), Some("nginx"));
    assert_eq!(
        request.categories,
        [
            ChangeCategory::ConfigChanged,
            ChangeCategory::ServiceRestarted
        ]
    );
    assert_eq!(request.limit, 10);
    assert_eq!(request.max_depth, 3);
}

#[test]
fn incident_flags_reject_unknown_categories_and_unbounded_queries() {
    for args in [
        vec!["--category", "invented"],
        vec!["--limit", "0"],
        vec!["--limit", "10001"],
        vec!["--max-depth", "5"],
    ] {
        let mut command = vec!["twin", "what-changed", "--since", "1h"];
        command.extend(args);
        assert!(Cli::try_parse_from(command).is_err());
    }
}

#[test]
fn test_run_selects_explicit_evaluation_context() {
    let cli = Cli::try_parse_from([
        "twin",
        "test",
        "run",
        "ops.yaml",
        "--max-age",
        "5m",
        "--baseline",
        "before",
        "--refresh",
    ])
    .expect("args");
    let Command::Test(args) = cli.command else {
        panic!("test command");
    };
    let twin_cli::cli::args::TestAction::Run(args) = args.action else {
        panic!("run command");
    };
    assert!(args.refresh);
    assert_eq!(args.max_age.as_deref(), Some("5m"));
    assert_eq!(args.baseline.as_deref(), Some("before"));
    assert!(
        Cli::try_parse_from(["twin", "test", "run", "--snapshot", "before", "--refresh"]).is_err()
    );
}
