# Slice 22: Docker Container Graph and Container Emulation

## Status: done

## Implemented

- Crate `twin-container`: read-only `DockerReadOnly` adapter (`list_containers`, `inspect_container`, `inspect_image`, `port_mappings`, `mounts`). No mutation methods.
- Fixture backend via `TWIN_DOCKER_FIXTURE` (`containers/list.json`, `containers/{id}.json`, `images/{reference}.json`) and a std-only HTTP GET client over the Docker unix socket for `/containers/json`, `/containers/{id}/json`, and `/images/{id}/json`.
- Graph nodes `container:{name}` and `image:{reference}` (`container:redis`, `image:redis:7`). Mount destinations reuse `NodeKind::Directory` (`directory:{path}`).
- Edges `runs_image`, `maps_port`, `mounts_volume`, and `owns` when the container pid is already a process node.
- `twin doctor --containers` reports socket presence and a read-only list attempt without crashing when Docker is absent.
- `twin graph container:redis` neighborhood and `twin emulate restart container:redis` via `RestartContainerOverlayBuilder`. Hypothetical only: `action_performed` is false, the overlay is not persisted, and risk is high when a service `connects_to` or `depends_on` a published host port.
- Safety test scans `crates/twin-container/src` (and the container command path) for Docker mutation API names.

## In Progress

_(nothing)_

## Blocked

_(nothing)_

## Decisions

- Container ids are `container:{name}` from the Docker name with the leading slash removed, not `container:docker:{name}`.
- Published ports use the existing `port:tcp:{ip}:{port}` id. Bind and volume mount destinations are `directory:` nodes so slice 20 directory ids stay consistent. No separate volume id.
- `RestartContainerOverlayBuilder` lives in `twin-emulate`. The adapter crate does not build overlays.
- Tests inject `TWIN_DOCKER_FIXTURE`. Live reads are used only when that variable is unset and `/var/run/docker.sock` exists.

## Deviations from Plan

- Live Docker uses a small HTTP/1.1 GET client on `std::os::unix::net::UnixStream` instead of a Docker SDK or HTTP crate. Only the three read paths above are allowed.
- The safety test bans Docker API mutation names and path markers (`ContainerRestart`, `ContainerStop`, `ContainerKill`, `ContainerRemove`, `ContainerExec`, `/containers/.*/start`, `/stop`, `/kill`, `/rename`). It does not ban the English word "restart" in emulation safety statements.
- `twin graph image:…` is not a dedicated neighborhood; image nodes are still stored and shown from the container view.

## 2026-10-08 follow-up: Docker fixtures and read status

`twin scan --docker-fixture DIR` explicitly selects the read-only fixture adapter. Legacy fixture environment selection remains compatible. Each scan records Docker success/unavailable status. Unavailable reads preserve existing Docker graph rows by excluding that source from finalization; availability reports the read outcome. containerd and Docker API negotiation remain unimplemented.

## 2026-10-09 integration fixtures

The health projection fixture includes a valid list membership record before inspect, and verifies that health log bodies are absent from the projected result. The local Unix API test gates only an actual PermissionDenied bind, emitting a runtime-capability explanation; other errors still fail and the full request assertion runs whenever local bind is supported.

## 2026-10-09 legacy Docker error cleanup

Schema 6 replaces stored Docker collector error messages with a fixed category and clears metadata for failed/error-bearing Docker runs so untrusted response bodies cannot remain queryable there. Synthetic sentinel regression checks both fields. Successful collector facts are preserved after generic argument-key cleanup. Existing users must run `twin init` for logical cleanup; backups, WAL remnants and exported artifacts are outside this guarantee.

## 2026-10-09 containerd and Docker API follow-up

Implemented a metadata-only containerd gRPC adapter over its Unix socket. The only RPCs are namespace/container/task/image `List`; OCI specifications, labels, task I/O and log values have no projected wire fields. Tonic/prost, hyper-util and tower provide the HTTP/2 transport; Tokio is reused for bounded sync wrappers and real async read APIs. Limits: 64 namespaces, 10,000 objects per list, 8 MiB messages, three seconds per RPC and a 15-second scan deadline checked between reads. There is no atomic cross-resource snapshot guarantee.

`scan --containerd-fixture DIR` supports deterministic offline coverage. Runtime nodes use namespace-qualified IDs (`container:containerd:default:api`), with task-to-process joins requiring a rechecked `/proc/PID/stat` start identity. Changed known creation timestamps or gone reappearance reset incarnation proof links. Missing identities stay unknown. Complete namespace reads finalize removed rows independently of unrelated host coverage gaps; partial reads mark missing relationships stale, and an unavailable runtime preserves its rows. Health policies scope containerd gaps separately from Docker gaps. `doctor --containers` now reports both runtime sockets/read outcomes, and scan coverage displays containerd availability.

Docker negotiates its API against `/version` before the existing GET-only calls. Its supported range is 1.24–1.51; an incompatible or malformed advertised range is a sanitized coverage failure. Neither adapter executes a container command or reads raw environment/specification values.

Validation: adapter fixture tests cover metadata projection, duplicate/path identifiers, malformed/missing resource lists and namespace separation. App tests cover disappearance during unrelated coverage loss, runtime outage, task process ownership and recreation proof reset. The protobuf projection test verifies unknown OCI/label/task-I/O bytes are discarded. CLI doctor output tests cover containerd paths and namespace counts. Live containerd HTTP/2 interoperability still needs a capable daemon or fake gRPC server; fixture tests do not establish live-daemon compatibility. Containerd port mappings and volume ownership are not inferred from excluded OCI specifications.
