use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRef {
    pub kind: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedResource {
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadView {
    pub namespace: String,
    pub name: String,
    pub owners: Vec<OwnerRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PodView {
    pub namespace: String,
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub owners: Vec<OwnerRef>,
    pub images: Vec<String>,
    pub config_map_names: Vec<String>,
    pub secret_names: Vec<String>,
    pub pvc_names: Vec<String>,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceView {
    pub namespace: String,
    pub name: String,
    pub selector: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointView {
    pub namespace: String,
    pub name: String,
    pub service_name: String,
    pub ready_pods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngressView {
    pub namespace: String,
    pub name: String,
    pub service_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventView {
    pub namespace: String,
    pub name: String,
    pub reason: String,
    pub involved_kind: String,
    pub involved_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClusterView {
    pub namespaces: Vec<String>,
    pub pods: Vec<PodView>,
    pub deployments: Vec<WorkloadView>,
    pub replicasets: Vec<WorkloadView>,
    pub services: Vec<ServiceView>,
    pub endpoints: Vec<EndpointView>,
    pub ingresses: Vec<IngressView>,
    pub configmaps: Vec<NamedResource>,
    pub secret_refs: Vec<NamedResource>,
    pub pvcs: Vec<NamedResource>,
    pub events: Vec<EventView>,
    pub warnings: Vec<String>,
}
