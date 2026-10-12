# Operational contracts and incident investigation

`twin` reads operational state and evaluates hypothetical overlays. It never performs the restart, rollout or deletion being rehearsed.

## Health contracts

Save the checks you need in `twin.yaml`. Replace these example IDs with IDs from your graph; omit checks for platforms you do not use.

```yaml
version: 1
name: application health
defaults:
  fail_on_unknown: true
checks:
  - name: API active
    assert:
      service: service:api.service
      active_state: active
  - name: API local listener
    assert:
      listener:
        port: port:tcp:127.0.0.1:8080
        owned_by: service:api.service
  - name: Container healthy
    assert:
      container:
        id: container:api
        healthy: true
  - name: Deployment ready and routed
    assert:
      k8s_ready:
        deployment: k8s:deployment:default/api
        routed_service: k8s:service:default/api
  - name: API peer policy
    assert:
      outbound_endpoints:
        from: service:api.service
        allowed: ['tcp:10.0.0.2:5432']
        fail_on_unknown: true
```

Service existence is inventory; `active_state` checks observed runtime state. A listener requires a local listener relationship, its requested owner, and retained supporting observations. Container checks use running state and the projected health status, never health log bodies. Kubernetes checks require current observed generations and readiness; routing uses ready endpoint pods and unambiguous deployment ownership. Missing, stale or incomplete relevant coverage produces Unknown. `fail_on_unknown` turns that uncertainty into a failed contract.

Transport-qualified allowlists distinguish TCP and UDP. Legacy `IP:port` entries allow either transport. Proc socket peers do not establish who initiated traffic; eBPF connect evidence can establish initiation. These contracts concern observed peers, not a firewall guarantee. Namespace endpoint IDs preserve namespace identity; a workload selector restricts evaluation to its attributed peers.

```bash
twin test run twin.yaml --refresh --max-age 5m
twin snapshot create healthy
twin test run twin.yaml --max-age 5m --baseline healthy
twin test run twin.yaml --snapshot healthy
```

`--refresh` refreshes host collection; Docker and Kubernetes have their separate scan commands. `--max-age` bounds acceptable graph evidence age. Current evaluations explicitly list live DNS/disk measurements. A snapshot evaluation uses captured graph rows, captured evidence references and collector coverage at the snapshot time; later incarnation replacement cannot substitute current evidence. Pruned observations become missing proof. Snapshots do not retain live disk measurements, DNS answers or complete emulation inputs: corresponding historical checks report Unknown. Older snapshots cannot recover proof links already discarded before migration.

Baseline output lists newly failing, newly uncertain and recovered checks, alongside the baseline check results. It compares operational contracts; it does not establish why a change happened.

## Investigate a failing workload

```bash
twin what-changed --since 1h --target api.service --limit 30 --max-depth 2
twin why api.service
twin impact api.service --paths --evidence
```

The incident timeline is chronological and follows bounded semantic relationships rather than broad mount or parent-process fan-out. Category filters preserve coverage unknowns and omitted-entry counts. Temporal association is not causation.

`why` preserves each link's observed/inferred class, supporting timestamps, safe facts and shared evidence strength. Proof traversal is bounded to four links, 64 relationships, 32 traversed nodes and 16 paths. Truncated display facts do not change the scoring evidence.

Restart and rollout predictions distinguish risk from confidence. Optional systemd Wants declarations alone do not establish runtime disruption. Kubernetes rolling-update predictions use observed ready endpoints and resolved policy budgets; they do not assume new surge pods become ready. Unsupported policy, missing endpoint coverage and stale observations produce Unknown availability.

## Existing installations

Run `twin init` to apply store migrations. Migration 6 removes legacy structured argument fields from observations, graph/history/snapshot metadata and retained structured test reports, and sanitizes retained Docker collector errors. This is logical cleanup in Twin's own database; it does not certify physical erasure from prior backups or filesystem/WAL remnants.

Namespace collection reads representative `/proc/PID/net` files without entering or altering namespaces. Permission failures and identity races remain visible as coverage gaps. Same-boot namespace inode reuse without an observed absence cannot be proved to be a new incarnation. Kubernetes layouts are bound to a cluster identity; use separate layouts for different clusters.

## Kubernetes resource continuity and log signals

```bash
twin k8s watch --duration 60 --max-events 256
twin k8s logs api-123 --namespace production --container web --previous --tail-lines 200 --max-bytes 65536 --timeout 5
```

Watch refreshes the stored Kubernetes graph from safe full reads after resource changes. It follows metadata resource versions, resumes from bookmarks and relists expired streams. The terminal report distinguishes changes, reconnects, relists and coverage gaps; it retains at most 256 notices. Duration is specified in seconds and bounded to one hour, with a separate bounded configuration setup. Watch uses the same cluster identity guard as scan.

Logs return observed volume and typed phrase counts, including connection-refused, timeout, permission-denied and out-of-memory matches. Log content is discarded because it may contain arbitrary credentials. Counts are case-insensitive textual observations within the requested fragment; they do not establish the cause of a problem. Byte/time limits and interrupted reads explicitly mark partial coverage. Logs never update the graph or store their response bodies; JSON returns the same safe aggregate facts as terminal output.

Containerd hosts are scanned through read-only namespace, container, task and image List RPCs. Run `twin doctor --containers` to check Docker and containerd separately. Containerd targets include their namespace: `twin graph container:containerd:default:api`. Task ownership requires a current matching process incarnation. OCI configuration, port mappings and volume declarations are excluded; missing runtime data remains Unknown. For offline scans use `twin scan --containerd-fixture DIR`, with `namespaces.json` and `namespaces/NAME/{containers,tasks,images}.json`.

Nginx includes are expanded in their enclosing HTTP/stream/upstream context, within bounded file/depth/byte limits. Stream TCP, UDP and Unix backends retain separate endpoint kinds. Concrete hostname backends resolve through the scanner's bounded system resolver; these current answers support inferred mappings and do not establish nginx's own resolver cache or namespace state. Failed resolution retains explicit uncertainty.
