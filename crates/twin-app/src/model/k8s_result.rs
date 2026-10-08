use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct K8sScanResult {
    pub source: String,
    pub kubeconfig_present: bool,
    pub kubeconfig_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_gap: Option<String>,
    pub warnings: Vec<String>,
    pub namespaces: usize,
    pub pods: usize,
    pub deployments: usize,
    pub replicasets: usize,
    pub services: usize,
    pub endpoints: usize,
    pub ingresses: usize,
    pub configmaps: usize,
    pub secret_refs: usize,
    pub pvcs: usize,
    pub events: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct K8sGraphResult {
    pub id: String,
    pub owns: Vec<String>,
    pub routed_by: Vec<String>,
    pub selects: Vec<String>,
    pub uses: Vec<String>,
    pub routes_to: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct K8sImpactResult {
    pub id: String,
    pub selects: Vec<String>,
    pub owned_by: Vec<String>,
    pub routed_by: Vec<String>,
    pub affected: Vec<String>,
}
