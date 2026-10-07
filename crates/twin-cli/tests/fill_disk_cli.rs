use twin_app::EmulateActionRequest;
use twin_cli::cli::args::EmulateFillDiskArgs;
use twin_cli::cli::emulate::emulate_fill_disk_request;

fn args(target: &str, to_percent: &str) -> EmulateFillDiskArgs {
    EmulateFillDiskArgs {
        config: None,
        target: target.to_string(),
        to_percent: to_percent.to_string(),
        evidence: false,
    }
}

#[test]
fn fill_disk_accepts_a_percent_suffix() {
    let request = emulate_fill_disk_request(&args("/var", "95%")).expect("request");
    match request.action {
        EmulateActionRequest::FillDisk { mount, to_percent } => {
            assert_eq!(mount, "/var");
            assert_eq!(to_percent, 95);
        }
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn fill_disk_rejects_percent_above_100() {
    let err = emulate_fill_disk_request(&args("/var", "140%")).expect_err("percent");
    assert!(err.to_string().contains("1% to 100%"), "{err}");
}
