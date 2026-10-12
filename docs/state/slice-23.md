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
- Host scans do not finalize k8s-sourced nodes or edges. `twin k8s scan` is the only writer of rows whose metadata source is `k8s`; a host scan refreshes them never. Before this, `twin k8s graph` reported "no kubernetes edges stored" and `twin emulate rollout` reported "deployment owns no pods" after any command that runs `refresh_scan` (graph, impact, emulate), because the host scan marked every k8s row stale. Regression test: `host_scan_does_not_stale_kubernetes_graph`.
- This is the last planned slice. There is no slice 24.

## Deviations from Plan

- `kube-rs` is not a dependency. There is no live GET client, no watch stream, and no pod log reader. Watch would force async through the crate, and log bodies can carry secret-like text.
- `twin doctor --k8s` checks kubeconfig presence. `twin k8s scan --fixture DIR` explicitly selects fixtures; the legacy fixture environment remains compatible. Without a fixture, bounded kube-rs live get/list reads run, and unavailable access returns a coverage gap.
- The kube-rs client is implemented. Log/watch product flows remain unimplemented; current adapters expose read-only get/list.

## 2026-10-08 follow-up: Kubernetes fixtures and failed reads

`twin k8s scan --fixture DIR` explicitly selects fixtures; legacy environment selection remains compatible. Config is validated. Missing/malformed fixture reads produce coverage gaps and unavailable collector status without reaping existing resources. Row labels display kind once followed by namespace/name. The live cluster client/watch/log reader remains unimplemented.

## Additional 2026-10-08 follow-up

Live Kubernetes scan is implemented with kube 4.2, k8s-openapi 0.28, and Tokio. TLS uses Rustls/ring. Resources use parallel GET/list, pagination (250/page, at most 1000 pages), 5-second request and 15-second whole-resource bounds. Secret and ConfigMap reads request metadata only and only names/references reach ClusterView. Error messages are sanitized; no server response bodies/credentials persist. Exec/auth-provider credential commands are rejected before client creation. Partial reads record degraded status and stale missing rows; total failure records unavailable and does not finalize. Fake HTTP API tests prove GET-only requests, pagination, secret-safe projection, permission errors, external-command rejection, and bounded timeout. Watch streams and pod-log reading remain unimplemented. The synchronous wrapper rejects invocation inside an existing runtime and offers read_cluster_async for that case.

## 2026-10-08 rollout availability reasoning

Deployment projection retains only safe rollout policy fields: desired replicas, Recreate vs RollingUpdate, and resolved maximum unavailable/surge counts. Kubernetes defaults and integer/percentage rounding are implemented, including controller fencepost rounding; malformed/unsupported policies warn without retaining raw values. ReplicaSet projections do not invent deployment policies.

Rollout compares observed service ready endpoints with owned pods and the rollout budget. Recreate may remove all owned ready endpoints; RollingUpdate reports a conditional lower bound using current ready endpoints, without assuming unobserved surge pods become ready. A rolling overlay does not mark every pod unavailable simultaneously: it cannot identify which pods are replaced together. All owned pods remain listed as replacement candidates. Missing policy, stale/missing reads, non-active resources or unknown endpoint coverage produce Unknown availability and weak evidence through the shared core scorer. Pod-delete reports similarly respect readiness coverage and age, and per-service impact statements use that service's remaining endpoints.

Read-only fixture tests cover Recreate, protected and disruptive rolling budgets, unknown coverage, stale evidence, rounding, malformed policy, surviving endpoints from another workload, and base-store preservation. These are conditional graph predictions, not a scheduling simulation: future readiness, capacity/quota, compatibility, probes/minReadySeconds and connection draining remain unknown. No mutation or live cluster is required.

Focused verification passed: twin-emulate K8s tests (9), twin-k8s projection/read tests (3), twin-app existing K8s tests (6) and rollout integration tests (2), plus strict all-targets Clippy for twin-app/twin-emulate/twin-k8s. Logs: `/tmp/twin-rollout-domain.log`, `/tmp/twin-rollout-parser.log`, `/tmp/twin-rollout-app.log`, `/tmp/twin-reasoning-clippy.log`. The malformed fixture retains its warning test and now asserts Unknown availability; the healthy lifecycle fixture includes a valid deployment spec.

## 2026-10-09 live-fixture runtime capability

Live API tests gate only an actual PermissionDenied from the local TCP fixture bind and emit a runtime-capability explanation. Other bind errors fail. Pagination, sanitized permission errors, timeout bounds and external authentication rejection remain exercised whenever the environment permits local listeners; no global ignore attributes or command skips were added.

## 2026-10-09 bounded resource watch and pod log summaries

Implemented `twin k8s watch --duration SECONDS --max-events N` and `twin k8s logs POD --namespace NS --container NAME --previous --tail-lines N --max-bytes N --timeout SECONDS`.

Watch opens metadata-only GET watches for all supported resource collections. Each collection first lists with pagination to obtain a resource version, resumes after EOF/errors from its latest event/bookmark version, and relists on expired (410) versions. Reconnects use bounded backoff. Events trigger safe full-cluster reads and transactional graph/history refreshes; interrupted watches record sourced coverage gaps and preserve graph state. External authentication commands remain rejected. Watch duration is 1–3600 seconds after configuration setup; event limit is 1–4096. The queue is bounded to 16 signals, retained notices to 256 and unique warning strings to 64; omitted counts remain visible. Stream events are parsed through a 64 KiB line bound; arbitrary metadata and raw malformed bodies are never displayed.

Pod logs use one bounded, non-following GET stream with server tail/byte limits and an independent client byte cap/time deadline (1–1048576 bytes, 1–10000 tail lines, 1–60 seconds). The result contains byte/line counts and typed case-insensitive lexical counts for connection-refused, timeout, permission-denied, out-of-memory, error and warning phrases. These are observed text matches, not assertions of health, cause or severity. No body, snippet, credential value or content hash is returned or persisted. Partial/denied/timed-out reads explicitly report unknown coverage. Transient chunks and the bounded lexical window are cleared after processing; no filesystem forensic erasure claim is made. Configuration inference has its separate five-second bound.

`futures-util` (already a kube transitive dependency) supplies stream I/O without enabling kube runtime mutation/watch abstractions. Configured client injection supports embedding and fake API integration without environment hooks. Shared fake GET/watch/log API fixtures live in twin-fixtures, a dev dependency; its serde_json dependency generates synthetic responses.

Verification: adapter fixtures prove GET-only metadata requests, bookmark continuation, expired-version relisting, bounded logs, sanitized denial/timeout, sentinel omission and request validation. A pure lexical helper regression spans arbitrary chunk boundaries. App tests prove graph refresh/interruption coverage and unchanged graph/observation storage for log reads. CLI argument/renderer tests lock in bounds, continuity, coverage and observed-phrase sections. Actual PermissionDenied loopback binding gates only the affected fake API tests; pure tests and renderers still run. Selected strict all-target Clippy passes in `/tmp/twin-k8s-stream-clippy.log`. Raw pod log display remains deliberately unavailable because arbitrary logs cannot be demonstrably free of secret values.

Whole-workspace integration verification: build, tests, strict all-target Clippy and fmt check pass in `/tmp/twin-gap-final-{build,tests,clippy,fmt}.log`. Tests report 665 passing functions across 132 suites. Ten fake-server cases return after actual PermissionDenied binding in the restricted runtime; 655 bodies run fully, none are ignored. The watch/bookmark/410 and bounded-log fake servers previously ran successfully before runtime permissions became restricted; current capability gates do not substitute for those live fixture assertions.
