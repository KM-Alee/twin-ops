# Slice 23: Kubernetes Read-Only Graph and Kubernetes Emulation

## Status: done

## Implemented

- Crate `twin-k8s`: read-only `K8sReadOnly` adapter with sync `get` and `list` for namespaces, pods, deployments, replicasets, services, endpoints or endpoint slices, ingresses, configmap metadata, secret references, PVCs, and events.
- Fixture backend via `TWIN_K8S_FIXTURE`, a directory of JSON or YAML list documents. Secret `data` and `stringData`, configmap `data`, and event message bodies are never copied into the model.
- Node ids `k8s:deployment:default/api`, `k8s:replicaset:default/api-abc`, `k8s:pod:default/api-123`, `k8s:service:default/api`, `k8s:ingress:default/api`, `k8s:configmap:default/api-config`, `k8s:secretref:default/api-tls`, `k8s:pvc:default/api-data`, plus namespace, endpoints, and event ids. CLI short forms `deployment/default/api`, `service/default/api`, `pod:default/api-123`, and `deployment:default/api` resolve to those ids.
- Edges `owns`, `selects`, `routes_to`, `uses_configmap`, `uses_secret_ref`, `uses_pvc`, and `runs_image`.
- `twin k8s scan`, `twin k8s graph`, and `twin k8s impact`. `twin doctor --k8s` reports whether a kubeconfig file exists.
- `twin emulate delete pod:default/api-123` via `DeleteK8sPodOverlayBuilder`. Risk is low when another ready endpoint remains and high when the delete would drop the last ready endpoint. `action_performed` is false.
- `twin emulate rollout deployment:default/api` via `RolloutK8sDeploymentOverlayBuilder`. Owned pods are listed as restart-affected. The overlay is not persisted and no rollout is performed.
- Safety test scans `crates/twin-k8s/src` for mutation API names (`DeleteParams`, `Api::delete`, `patch`, `replace`, `create`, `exec`, `Attach`, `portforward`, `scale`).

## In Progress

_(nothing)_

## Blocked

_(nothing)_

## Decisions

- `twin-k8s` does not depend on `twin-core`. Graph ids and edges are built in `twin-app`.
- The kubeconfig presence check lives in `config_probe.rs`. The repo gitignore drops paths named `kubeconfig.*`, so that filename cannot hold source.
- Readiness for pod-delete risk prefers Endpoints or EndpointSlice `targetRef` pod names. When no endpoint document exists, ready pods are selector matches whose Ready condition is true.
- Deployment ownership is the ReplicaSet `ownerReferences` edge. ReplicaSet ownership is the Pod `ownerReferences` edge. Service selection is the label selector. Ingress routing reads `backend.service.name` and the older `serviceName` field.
- Malformed fixture documents add a warning that names the file and omits the parser snippet, so a broken document cannot copy secret-like bytes into the warning.
- This is the last planned slice. There is no slice 24.

## Deviations from Plan

- `kube-rs` is not a dependency. There is no live GET client, no watch stream, and no pod log reader. Watch would force async through the crate, and log bodies can carry secret-like text.
- `twin doctor --k8s` checks kubeconfig presence (`KUBECONFIG` or `~/.kube/config`) only. `twin k8s scan` reads `TWIN_K8S_FIXTURE` when set. When the fixture is unset, scan returns a coverage gap and the command exits 0, including when a kubeconfig file exists, because this build does not contact a cluster.
- The tech document's `kube-rs` client and log/watch verbs are therefore not implemented. The adapter trait still exposes only get and list.
