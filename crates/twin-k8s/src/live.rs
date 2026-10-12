use std::path::Path;
use std::time::Duration;

use kube::api::{ApiResource, DynamicObject, GroupVersionKind, ListParams};
use kube::{Api, Client, Config};
use serde_json::Value;

use crate::{ClusterView, K8sError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const RESOURCES: &[(&str, &str, &str, &str, bool)] = &[
    ("", "v1", "Namespace", "namespaces", true),
    ("", "v1", "Pod", "pods", false),
    ("apps", "v1", "Deployment", "deployments", false),
    ("apps", "v1", "ReplicaSet", "replicasets", false),
    ("", "v1", "Service", "services", false),
    ("", "v1", "Endpoints", "endpoints", false),
    (
        "discovery.k8s.io",
        "v1",
        "EndpointSlice",
        "endpointslices",
        false,
    ),
    ("networking.k8s.io", "v1", "Ingress", "ingresses", false),
    ("", "v1", "ConfigMap", "configmaps", true),
    ("", "v1", "Secret", "secrets", true),
    (
        "",
        "v1",
        "PersistentVolumeClaim",
        "persistentvolumeclaims",
        true,
    ),
    ("", "v1", "Event", "events", false),
];

fn unavailable(detail: &str) -> K8sError {
    K8sError::Client {
        detail: detail.into(),
    }
}

pub fn read_cluster() -> Result<ClusterView, K8sError> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(unavailable(
            "use read_cluster_async inside an async runtime",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| unavailable("cannot initialize the read runtime"))?;
    runtime.block_on(async {
        let config = tokio::time::timeout(REQUEST_TIMEOUT, Config::infer())
            .await
            .map_err(|_| unavailable("configuration loading timed out"))?
            .map_err(|_| unavailable("no usable kubeconfig or in-cluster configuration"))?;
        read_cluster_async(config).await
    })
}

pub(crate) async fn infer_config() -> Result<Config, K8sError> {
    tokio::time::timeout(REQUEST_TIMEOUT, Config::infer())
        .await
        .map_err(|_| unavailable("configuration loading timed out"))?
        .map_err(|_| unavailable("no usable kubeconfig or in-cluster configuration"))
}

pub(crate) fn client_from_config(mut config: Config) -> Result<Client, K8sError> {
    // Credential plugins may run arbitrary host commands before making a read.
    if config.auth_info.exec.is_some() || config.auth_info.auth_provider.is_some() {
        return Err(unavailable(
            "external credential commands are disabled; use certificate or token authentication",
        ));
    }
    config.connect_timeout = Some(REQUEST_TIMEOUT);
    config.read_timeout = Some(REQUEST_TIMEOUT);
    config.write_timeout = Some(REQUEST_TIMEOUT);
    Client::try_from(config).map_err(|_| unavailable("cannot configure TLS or authentication"))
}

pub async fn read_cluster_async(config: Config) -> Result<ClusterView, K8sError> {
    let source_identity = source_identity(&config);
    let client = client_from_config(config)?;
    let mut reads = tokio::task::JoinSet::new();
    for &(group, version, kind, plural, metadata_only) in RESOURCES {
        let client = client.clone();
        reads.spawn(async move {
            let result = tokio::time::timeout(
                Duration::from_secs(15),
                read_resource(client, group, version, kind, plural, metadata_only),
            )
            .await
            .unwrap_or(Err("resource listing timed out"));
            (kind, plural, result)
        });
    }
    let mut results = Vec::new();
    while let Some(read) = reads.join_next().await {
        match read {
            Ok(result) => results.push(result),
            Err(_) => return Err(unavailable("resource reader failed")),
        }
    }
    results.sort_by_key(|(kind, _, _)| *kind);
    let mut view = ClusterView {
        source_identity: Some(source_identity),
        ..ClusterView::default()
    };
    let mut successful = 0usize;
    for (kind, plural, result) in results {
        match result {
            Ok(items) => {
                successful += 1;
                if kind == "Endpoints" || kind == "EndpointSlice" {
                    view.endpoints_complete = true;
                }
                for item in items {
                    crate::parse::ingest(Path::new(plural), &item, Some(kind), &mut view);
                }
            }
            Err(detail) => view.warnings.push(format!("Kubernetes {plural}: {detail}")),
        }
    }
    if successful == 0 {
        return Err(unavailable("all resource reads failed (permissions, network, or API availability); no graph rows were finalized"));
    }
    Ok(view)
}

pub(crate) fn source_identity(config: &Config) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in config
        .cluster_url
        .to_string()
        .bytes()
        .chain(config.root_cert.iter().flatten().flatten().copied())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("api-{hash:016x}")
}

async fn read_resource(
    client: Client,
    group: &str,
    version: &str,
    kind: &str,
    plural: &str,
    metadata_only: bool,
) -> Result<Vec<Value>, &'static str> {
    let mut resource = ApiResource::from_gvk(&GroupVersionKind::gvk(group, version, kind));
    resource.plural = plural.into();
    let api: Api<DynamicObject> = Api::all_with(client, &resource);
    let mut params = ListParams::default().limit(250);
    let mut items = Vec::new();
    for _ in 0..1000 {
        let (page, next) = if metadata_only {
            let list = tokio::time::timeout(REQUEST_TIMEOUT, api.list_metadata(&params))
                .await
                .map_err(|_| "read timed out")?
                .map_err(read_error)?;
            let page = list
                .items
                .into_iter()
                .map(|item| serde_json::json!({ "metadata": item.metadata }))
                .collect::<Vec<_>>();
            (page, list.metadata.continue_)
        } else {
            let list = tokio::time::timeout(REQUEST_TIMEOUT, api.list(&params))
                .await
                .map_err(|_| "read timed out")?
                .map_err(read_error)?;
            let page = list
                .items
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "invalid resource response")?;
            (page, list.metadata.continue_)
        };
        items.extend(page);
        let Some(next) = next.filter(|next| !next.is_empty()) else {
            return Ok(items);
        };
        if params.continue_token.as_ref() == Some(&next) {
            return Err("pagination did not advance");
        }
        params = params.continue_token(&next);
    }
    Err("pagination safety limit reached; incomplete resource list discarded")
}

pub(crate) fn read_error(error: kube::Error) -> &'static str {
    match error {
        kube::Error::Api(response) if response.code == 401 || response.code == 403 => {
            "read permission denied"
        }
        kube::Error::Api(response) if response.code == 404 => "resource API unavailable",
        _ => "read failed (network, TLS, or invalid API response)",
    }
}
