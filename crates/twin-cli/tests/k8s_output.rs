use twin_app::{
    EmulateActionRequest, EmulationImpact, EmulationResult, K8sGraphResult, K8sImpactResult,
    K8sScanResult,
};
use twin_cli::cli::args::{EmulateDeleteArgs, EmulateRolloutArgs};
use twin_cli::cli::emulate::{emulate_delete_request, emulate_rollout_request};
use twin_cli::output;
use twin_core::RiskLevel;

fn sample_delete() -> EmulationResult {
    let mut result = EmulationResult {
        action: "delete".to_string(),
        target: "k8s:pod:default/api-123".to_string(),
        target_label: "pod:default/api-123".to_string(),
        action_performed: false,
        safety_statement: "No pod was deleted.".to_string(),
        general_safety_statement: "No action was performed.".to_string(),
        risk: twin_app::RiskAssessment {
            level: RiskLevel::Low,
            reasons: vec!["service still has 1 ready endpoint.".to_string()],
        },
        evidence_strength: twin_app::EvidenceStrengthView {
            score: 45,
            label: "moderate".to_string(),
        },
        overlay: twin_app::EmulationOverlaySummary {
            unavailable_nodes: vec![],
            interrupted_relationships: vec![],
        },
        transient_impacts: vec![],
        configured_impacts: vec![],
        runtime_impacts: vec![],
        restart_impacts: vec![],
        persistent_impacts: vec![],
        unknown_impacts: vec![],
        evidence_lines: vec![],
        impact_paths: vec![],
        paths_requested: false,
        max_depth: 1,
        evidence_reasons: vec![],
        unknowns: vec![],
        show_evidence: false,
    };
    let _ = &mut result;
    result
}

#[test]
fn k8s_graph_and_impact_sections() {
    let graph = output::k8s::render_graph(&K8sGraphResult {
        id: "k8s:deployment:default/api".to_string(),
        owns: vec![
            "replicaset:default/api-abc".to_string(),
            "pod:default/api-123".to_string(),
            "pod:default/api-124".to_string(),
        ],
        routed_by: vec![
            "service:default/api".to_string(),
            "ingress:default/api".to_string(),
        ],
        selects: vec![],
        uses: vec![],
        routes_to: vec![],
    });
    assert!(graph.contains("k8s:deployment:default/api"));
    assert!(graph.contains("Owns"));
    assert!(graph.contains("replicaset:default/api-abc"));
    assert!(graph.contains("pod:default/api-123"));
    assert!(graph.contains("Routed by"));
    assert!(graph.contains("service:default/api"));
    assert!(graph.contains("ingress:default/api"));
    assert!(graph.contains("└──"));

    let impact = output::k8s::render_impact(&K8sImpactResult {
        id: "k8s:service:default/api".to_string(),
        selects: vec![
            "pod:default/api-123".to_string(),
            "pod:default/api-124".to_string(),
        ],
        owned_by: vec!["deployment:default/api".to_string()],
        routed_by: vec!["ingress:default/api".to_string()],
        affected: vec!["pod:default/api-123".to_string()],
    });
    assert!(impact.contains("Selects"));
    assert!(impact.contains("Owned by"));
    assert!(impact.contains("deployment:default/api"));
}

#[test]
fn k8s_emulation_output_states_risk_and_safety() {
    let deleted = output::emulate::render(&sample_delete());
    assert!(deleted.contains("Emulation: delete pod:default/api-123"));
    assert!(deleted.contains("Risk: LOW"));
    assert!(deleted.contains("service still has 1 ready endpoint."));
    assert!(deleted.contains("No pod was deleted."));

    let mut rollout = sample_delete();
    rollout.action = "rollout".to_string();
    rollout.target = "k8s:deployment:default/api".to_string();
    rollout.target_label = "deployment:default/api".to_string();
    rollout.safety_statement = "No deployment was rolled out.".to_string();
    rollout.risk.reasons = vec!["hypothetical rollout restarts 2 owned pods.".to_string()];
    rollout.restart_impacts = vec![
        EmulationImpact {
            id: "k8s:pod:default/api-123".to_string(),
            label: "api-123".to_string(),
            statement: "pod:default/api-123 would restart".to_string(),
            path: String::new(),
            evidence: vec![],
        },
        EmulationImpact {
            id: "k8s:pod:default/api-124".to_string(),
            label: "api-124".to_string(),
            statement: "pod:default/api-124 would restart".to_string(),
            path: String::new(),
            evidence: vec![],
        },
    ];
    let text = output::emulate::render(&rollout);
    assert!(text.contains("Emulation: rollout deployment:default/api"));
    assert!(text.contains("Restart-affected"));
    assert!(text.contains("pod:default/api-123"));
    assert!(text.contains("No deployment was rolled out."));
}

#[test]
fn k8s_scan_warning_stays_in_the_tree() {
    let text = output::k8s::render_scan(&K8sScanResult {
        source: "fixture".to_string(),
        kubeconfig_present: false,
        kubeconfig_path: "/tmp/kubeconfig".to_string(),
        coverage_gap: None,
        warnings: vec!["broken.yaml: malformed document".to_string()],
        namespaces: 1,
        pods: 2,
        deployments: 1,
        replicasets: 1,
        services: 1,
        endpoints: 1,
        ingresses: 1,
        configmaps: 1,
        secret_refs: 1,
        pvcs: 1,
        events: 1,
    });
    assert!(text.contains("twin k8s scan"));
    assert!(text.contains("warnings"));
    assert!(text.contains("malformed document"));
    assert!(text.contains("secret refs"));
}

#[test]
fn delete_file_paths_stay_file_targets() {
    let file = emulate_delete_request(&EmulateDeleteArgs {
        config: None,
        target: "/etc/nginx/nginx.conf".to_string(),
        evidence: false,
    })
    .expect("file");
    assert!(matches!(
        file.action,
        EmulateActionRequest::DeleteFile { .. }
    ));
    let pod = emulate_delete_request(&EmulateDeleteArgs {
        config: None,
        target: "pod:default/api-123".to_string(),
        evidence: false,
    })
    .expect("pod");
    match pod.action {
        EmulateActionRequest::DeleteK8sPod { target } => {
            assert_eq!(target.as_str(), "k8s:pod:default/api-123");
        }
        other => panic!("unexpected {other:?}"),
    }
    let rollout = emulate_rollout_request(&EmulateRolloutArgs {
        config: None,
        target: "deployment:default/api".to_string(),
        evidence: false,
    })
    .expect("rollout");
    match rollout.action {
        EmulateActionRequest::RolloutK8sDeployment { target } => {
            assert_eq!(target.as_str(), "k8s:deployment:default/api");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn doctor_k8s_section_mentions_read_only_mode() {
    let mut result = twin_cli::output::doctor::render(&{
        let mut doctor = sample_doctor();
        doctor.k8s = Some(twin_app::DoctorK8s {
            kubeconfig_present: false,
            kubeconfig_path: "/home/ubuntu/.kube/config".to_string(),
            detail: "kubeconfig not found. Read-only get/list only.".to_string(),
        });
        doctor
    });
    assert!(result.contains("kubernetes"));
    assert!(result.contains("kubeconfig"));
    assert!(result.contains("Read-only get/list only."));
    let _ = &mut result;
}

fn sample_doctor() -> twin_app::DoctorResult {
    twin_app::DoctorResult {
        core: twin_app::DoctorCore {
            cli_ok: true,
            config_found: true,
            config_path: None,
        },
        database: twin_app::DoctorDatabase {
            initialized: true,
            schema_version: Some(1),
            db_path: None,
            wal_mode: Some(true),
        },
        permissions: twin_app::DoctorPermissions {
            mode: twin_app::PermissionMode::Unprivileged,
            proc_accessible: true,
            readable_process_count: Some(1),
            restricted_process_count: Some(0),
        },
        scan_quality: None,
        scan_quality_error: None,
        ebpf: None,
        containers: None,
        k8s: None,
    }
}
