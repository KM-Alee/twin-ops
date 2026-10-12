use std::path::{Path, PathBuf};
use std::time::Duration;

use hyper_util::rt::TokioIo;
use prost::Message;
use tonic::transport::Endpoint;
use tonic::{Request, Status};

use super::wire;
use super::{
    ContainerMetadata, ContainerdError, ContainerdReadOnly, ImageMetadata, TaskMetadata, TaskState,
};

const RPC_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_MESSAGE: usize = 8 * 1024 * 1024;

pub struct LiveContainerd {
    socket: PathBuf,
}

impl LiveContainerd {
    pub fn new(socket: &Path) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    async fn list_async<T>(
        &self,
        namespace: Option<&str>,
        path: &'static str,
        operation: &'static str,
    ) -> Result<T, ContainerdError>
    where
        T: Message + Default + Send + 'static,
    {
        if namespace.is_some_and(|namespace| !super::valid_identifier(namespace)) {
            return Err(rpc_error(operation, "invalid namespace"));
        }
        tokio::time::timeout(RPC_TIMEOUT, async {
            let socket = self.socket.clone();
            let channel = Endpoint::from_static("http://[::]:50051")
                .connect_timeout(RPC_TIMEOUT)
                .timeout(RPC_TIMEOUT)
                .connect_with_connector(tower::service_fn(move |_| {
                    let socket = socket.clone();
                    async move {
                        tokio::net::UnixStream::connect(socket)
                            .await
                            .map(TokioIo::new)
                    }
                }))
                .await
                .map_err(|_| rpc_error(operation, "socket unavailable or access denied"))?;
            let mut grpc = tonic::client::Grpc::new(channel).max_decoding_message_size(MAX_MESSAGE);
            grpc.ready()
                .await
                .map_err(|_| rpc_error(operation, "transport unavailable"))?;
            let mut request = Request::new(wire::Empty {});
            request.set_timeout(RPC_TIMEOUT);
            if let Some(namespace) = namespace {
                let value = namespace
                    .parse()
                    .map_err(|_| rpc_error(operation, "invalid namespace"))?;
                request.metadata_mut().insert("containerd-namespace", value);
            }
            let response: tonic::Response<T> = grpc
                .unary(
                    request,
                    path.parse()
                        .map_err(|_| rpc_error(operation, "invalid read path"))?,
                    tonic_prost::ProstCodec::default(),
                )
                .await
                .map_err(|status| status_error(operation, status))?;
            Ok(response.into_inner())
        })
        .await
        .map_err(|_| rpc_error(operation, "read deadline exceeded"))?
    }
}

impl LiveContainerd {
    pub async fn list_namespaces(&self) -> Result<Vec<String>, ContainerdError> {
        let result: wire::Namespaces = self
            .list_async(
                None,
                "/containerd.services.namespaces.v1.Namespaces/List",
                "namespaces.list",
            )
            .await?;
        Ok(result
            .namespaces
            .into_iter()
            .map(|namespace| namespace.name)
            .collect())
    }
    pub async fn list_containers(
        &self,
        namespace: &str,
    ) -> Result<Vec<ContainerMetadata>, ContainerdError> {
        let result: wire::Containers = self
            .list_async(
                Some(namespace),
                "/containerd.services.containers.v1.Containers/List",
                "containers.list",
            )
            .await?;
        Ok(result
            .containers
            .into_iter()
            .map(|container| ContainerMetadata {
                id: container.id,
                image: (!container.image.is_empty()).then_some(container.image),
                created_at_ns: container.created_at.and_then(|at| {
                    if !(0..1_000_000_000).contains(&at.nanos) {
                        return None;
                    }
                    at.seconds
                        .checked_mul(1_000_000_000)?
                        .checked_add(i64::from(at.nanos))
                }),
            })
            .collect())
    }
    pub async fn list_tasks(&self, namespace: &str) -> Result<Vec<TaskMetadata>, ContainerdError> {
        let result: wire::Tasks = self
            .list_async(
                Some(namespace),
                "/containerd.services.tasks.v1.Tasks/List",
                "tasks.list",
            )
            .await?;
        Ok(result
            .tasks
            .into_iter()
            .map(|task| TaskMetadata {
                container_id: task.container_id,
                pid: (task.pid > 0).then_some(task.pid),
                state: TaskState::from_wire(task.status),
            })
            .collect())
    }
    pub async fn list_images(
        &self,
        namespace: &str,
    ) -> Result<Vec<ImageMetadata>, ContainerdError> {
        let result: wire::Images = self
            .list_async(
                Some(namespace),
                "/containerd.services.images.v1.Images/List",
                "images.list",
            )
            .await?;
        Ok(result
            .images
            .into_iter()
            .map(|image| ImageMetadata {
                name: image.name,
                digest: image
                    .target
                    .map(|target| target.digest)
                    .filter(|digest| !digest.is_empty()),
            })
            .collect())
    }
}

impl ContainerdReadOnly for LiveContainerd {
    fn list_namespaces(&self) -> Result<Vec<String>, ContainerdError> {
        wait("namespaces.list", self.list_namespaces())
    }
    fn list_containers(&self, namespace: &str) -> Result<Vec<ContainerMetadata>, ContainerdError> {
        wait("containers.list", self.list_containers(namespace))
    }
    fn list_tasks(&self, namespace: &str) -> Result<Vec<TaskMetadata>, ContainerdError> {
        wait("tasks.list", self.list_tasks(namespace))
    }
    fn list_images(&self, namespace: &str) -> Result<Vec<ImageMetadata>, ContainerdError> {
        wait("images.list", self.list_images(namespace))
    }
}

fn wait<T>(
    operation: &'static str,
    future: impl std::future::Future<Output = Result<T, ContainerdError>>,
) -> Result<T, ContainerdError> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(rpc_error(
            operation,
            "use the asynchronous reader inside a runtime",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| rpc_error(operation, "runtime unavailable"))?;
    runtime.block_on(future)
}

fn status_error(operation: &'static str, status: Status) -> ContainerdError {
    rpc_error(
        operation,
        match status.code() {
            tonic::Code::PermissionDenied | tonic::Code::Unauthenticated => "permission denied",
            tonic::Code::DeadlineExceeded => "read deadline exceeded",
            tonic::Code::Unimplemented => "RPC unsupported",
            tonic::Code::NotFound => "resource not found",
            _ => "RPC failed",
        },
    )
}

fn rpc_error(operation: &'static str, category: &str) -> ContainerdError {
    ContainerdError::Rpc {
        operation,
        category: category.into(),
    }
}
