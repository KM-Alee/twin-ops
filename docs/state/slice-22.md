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
