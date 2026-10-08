mod adapter;
mod error;
mod fixture;
mod config_probe;
mod model;

pub use adapter::{cluster_view, open_source, K8sReadOnly, OpenSource};
pub use error::K8sError;
pub use fixture::FixtureK8s;
pub use config_probe::{kubeconfig_check, KubeconfigCheck};
pub use model::{
    ClusterView, EndpointView, EventView, IngressView, NamedResource, OwnerRef, PodView,
    ServiceView, WorkloadView,
};
