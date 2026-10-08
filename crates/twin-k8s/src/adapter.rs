use std::path::Path;

use crate::error::K8sError;
use crate::fixture::FixtureK8s;
use crate::model::{
    ClusterView, EndpointView, EventView, IngressView, NamedResource, PodView, ServiceView,
    WorkloadView,
};

pub trait K8sReadOnly {
    fn list_namespaces(&self) -> Result<Vec<String>, K8sError>;
    fn list_pods(&self) -> Result<Vec<PodView>, K8sError>;
    fn list_deployments(&self) -> Result<Vec<WorkloadView>, K8sError>;
    fn list_replicasets(&self) -> Result<Vec<WorkloadView>, K8sError>;
    fn list_services(&self) -> Result<Vec<ServiceView>, K8sError>;
    fn list_endpoints(&self) -> Result<Vec<EndpointView>, K8sError>;
    fn list_ingresses(&self) -> Result<Vec<IngressView>, K8sError>;
    fn list_configmaps(&self) -> Result<Vec<NamedResource>, K8sError>;
    fn list_secret_refs(&self) -> Result<Vec<NamedResource>, K8sError>;
    fn list_pvcs(&self) -> Result<Vec<NamedResource>, K8sError>;
    fn list_events(&self) -> Result<Vec<EventView>, K8sError>;

    fn get_namespace(&self, name: &str) -> Result<Option<String>, K8sError> {
        Ok(self
            .list_namespaces()?
            .into_iter()
            .find(|item| item == name))
    }

    fn get_pod(&self, namespace: &str, name: &str) -> Result<Option<PodView>, K8sError> {
        Ok(self
            .list_pods()?
            .into_iter()
            .find(|item| item.namespace == namespace && item.name == name))
    }

    fn get_deployment(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<WorkloadView>, K8sError> {
        Ok(self
            .list_deployments()?
            .into_iter()
            .find(|item| item.namespace == namespace && item.name == name))
    }

    fn get_replicaset(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<WorkloadView>, K8sError> {
        Ok(self
            .list_replicasets()?
            .into_iter()
            .find(|item| item.namespace == namespace && item.name == name))
    }

    fn get_service(&self, namespace: &str, name: &str) -> Result<Option<ServiceView>, K8sError> {
        Ok(self
            .list_services()?
            .into_iter()
            .find(|item| item.namespace == namespace && item.name == name))
    }

    fn get_endpoints(&self, namespace: &str, name: &str) -> Result<Option<EndpointView>, K8sError> {
        Ok(self
            .list_endpoints()?
            .into_iter()
            .find(|item| item.namespace == namespace && item.name == name))
    }

    fn get_ingress(&self, namespace: &str, name: &str) -> Result<Option<IngressView>, K8sError> {
        Ok(self
            .list_ingresses()?
            .into_iter()
            .find(|item| item.namespace == namespace && item.name == name))
    }

    fn get_configmap(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<NamedResource>, K8sError> {
        find_named(self.list_configmaps()?, namespace, name)
    }

    fn get_secret_ref(
        &self,
        namespace: &str,
        name: &str,
    ) -> Result<Option<NamedResource>, K8sError> {
        find_named(self.list_secret_refs()?, namespace, name)
    }

    fn get_pvc(&self, namespace: &str, name: &str) -> Result<Option<NamedResource>, K8sError> {
        find_named(self.list_pvcs()?, namespace, name)
    }

    fn get_event(&self, namespace: &str, name: &str) -> Result<Option<EventView>, K8sError> {
        Ok(self
            .list_events()?
            .into_iter()
            .find(|item| item.namespace == namespace && item.name == name))
    }
}

fn find_named(
    items: Vec<NamedResource>,
    namespace: &str,
    name: &str,
) -> Result<Option<NamedResource>, K8sError> {
    Ok(items
        .into_iter()
        .find(|item| item.namespace == namespace && item.name == name))
}

impl K8sReadOnly for FixtureK8s {
    fn list_namespaces(&self) -> Result<Vec<String>, K8sError> {
        Ok(self.view().namespaces.clone())
    }

    fn list_pods(&self) -> Result<Vec<PodView>, K8sError> {
        Ok(self.view().pods.clone())
    }

    fn list_deployments(&self) -> Result<Vec<WorkloadView>, K8sError> {
        Ok(self.view().deployments.clone())
    }

    fn list_replicasets(&self) -> Result<Vec<WorkloadView>, K8sError> {
        Ok(self.view().replicasets.clone())
    }

    fn list_services(&self) -> Result<Vec<ServiceView>, K8sError> {
        Ok(self.view().services.clone())
    }

    fn list_endpoints(&self) -> Result<Vec<EndpointView>, K8sError> {
        Ok(self.view().endpoints.clone())
    }

    fn list_ingresses(&self) -> Result<Vec<IngressView>, K8sError> {
        Ok(self.view().ingresses.clone())
    }

    fn list_configmaps(&self) -> Result<Vec<NamedResource>, K8sError> {
        Ok(self.view().configmaps.clone())
    }

    fn list_secret_refs(&self) -> Result<Vec<NamedResource>, K8sError> {
        Ok(self.view().secret_refs.clone())
    }

    fn list_pvcs(&self) -> Result<Vec<NamedResource>, K8sError> {
        Ok(self.view().pvcs.clone())
    }

    fn list_events(&self) -> Result<Vec<EventView>, K8sError> {
        Ok(self.view().events.clone())
    }
}

pub enum OpenSource {
    Fixture(Box<FixtureK8s>),
    Gap { detail: String },
}

pub fn open_source() -> OpenSource {
    match std::env::var("TWIN_K8S_FIXTURE") {
        Ok(value) if !value.is_empty() => match FixtureK8s::open(Path::new(&value)) {
            Ok(fixture) => OpenSource::Fixture(Box::new(fixture)),
            Err(err) => OpenSource::Gap {
                detail: err.to_string(),
            },
        },
        _ => OpenSource::Gap {
            detail: "no TWIN_K8S_FIXTURE directory is set".to_string(),
        },
    }
}

pub fn cluster_view(source: &OpenSource) -> Option<&ClusterView> {
    match source {
        OpenSource::Fixture(fixture) => Some(fixture.view()),
        OpenSource::Gap { .. } => None,
    }
}
