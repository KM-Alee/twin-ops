# Slice 5: Cgroups and systemd Service Mapping

## Status: complete

## Implemented

- `/proc/<pid>/cgroup` parsing (cgroup v1 and v2) in `twin-collectors`
- `ProcessBelongsToCgroup` observations with `ProcCgroup` source
- Cgroup and service graph nodes with conservative `.service` unit inference
- Observed `process -> cgroup` `in_cgroup` edges and inferred `service -> process|cgroup` `owns` edges
- Scan persistence with evidence links (`direct` / `support`)
- Extended `ScanResult` counts and CLI scan output
- Service graph neighborhood (`twin graph service:nginx.service`, `twin graph nginx`)
- `--kind service` list view
- Collector, core, app, and CLI E2E tests (`cli_integration.rs`, `TWIN_PROC_ROOT`)
- `sudo` uses invoking user's XDG data dir via `SUDO_USER` / `SUDO_UID`

## Decisions

- Extended `ProcessCollector` in-process rather than a separate collector trait
- Service ownership is `EdgeClass::Inferred` with metadata `systemd_cgroup_path`
- Graph service name resolution lives in `twin-app` (`target_query` from CLI for shorthand names)
- No systemd D-Bus or unit-file parsing in this slice
- Cgroup path canonicalization uses `twin_core::lexical_canonical` (same as `NodeId::cgroup`)
- `TWIN_PROC_ROOT` env for CLI integration tests only (not user-facing)

## Post-ship cleanup (simplify pass)

- Single-pass `parse_cgroup_memberships` (no triple parse in procfs)
- `scan.rs` helpers: `upsert_node_once`, `load_existing_edge`, `upsert_edge_with_link`
- `HashSet<NodeId>` for seen cgroup/service nodes; one cgroup observation lookup per membership
- `GraphEdge::inferred_service_owns` public; scan uses it for both owns targets
- Warning JSON-detail policy test moved to `twin-collectors/tests/warning.rs`

## Deferred (not in simplify pass)

- Shared `write_proc_fixture_*` across app/collectors/cli test crates (needs `twin-fixtures` crate)
- In-transaction node/edge caches or SQL upsert without pre-`get_edge` (scan perf at host scale)
- `get_edge_typed` on store

## Deviations from Plan

- None

Follow-up: Store::get_edge_typed mirrors get_node_typed and returns decoding errors instead of silently discarding malformed edge rows. ScanEdgeCache uses it. The shared fake-proc file writer now lives in twin-fixtures/src/procfs.rs; app/CLI/collector test support delegates to it. Broader transaction node caches remain performance work.


The generic graph/export neighborhood now batches peer-node decoding and edge-observation lookups in bounded SQL chunks, preserving labels and evidence without one query per neighbor. Store tests cover empty/missing IDs, parameter binding, duplicate IDs across multiple chunk boundaries, and typed-edge decode errors. Specialized neighborhoods are covered by the follow-up below; broader transaction node caching remains a performance follow-up.


Unfinalized-row queries are separate from active-only current/snapshot queries. Finalization can resolve stale rows after coverage recovers; repeated stale/gone transitions remain idempotent.

## 2026-10-09 namespace topology review

Process socket collection reads unique observed network namespaces through `/proc/PID/net` without `setns` or host changes. Namespace socket ownership joins only descriptors from processes in that namespace; identical endpoint addresses and inode numbers in different namespaces remain separate. Scanner namespace endpoints retain existing host IDs; other internet endpoints carry `:netns:INODE`, and Unix endpoints use `unix:netns:INODE:PATH`. Namespace identities are reread after collection; changed/vanished identities discard the socket sample and record a coverage gap. Table warning and evidence references identify the actual PID namespace source. FD observations include namespace identity.

Wildcard listeners support inferred local dependencies only for loopback or concrete local addresses observed in the same namespace, never arbitrary remote internet peers. Namespaced edges retain the normal relation-specific inference metadata and observation links; Unix sockets do not acquire TCP metadata. Missing namespace/table evidence preserves uncertainty rather than asserting disappearance. Fixture verification: `twin-app/tests/network_namespaces.rs` covers duplicate endpoints/inodes, lost identity, host/Unix separation and remote wildcard rejection; `twin-collectors/tests/network_namespaces.rs` covers host/non-host identity changes and vanishing identities. Known boot changes reset endpoint and runtime-edge incarnation boundaries while retaining canonical aliases, close prior runtime edges, and remove previous-incarnation evidence links. Unknown boot reads produce coverage gaps and boot_identity_known=false; the last known boot is retained solely to detect a later boundary. Reappearing gone endpoints reset their lifetime. Namespace inode reuse without an observed absence within the same boot remains a kernel identity limit; these endpoint IDs are not permanent workload identities. Completely missing identities suppress all guessed host ownership and record coverage gaps. Scan counts include namespaced endpoints, relationships, sockets and namespace-scoped inode totals.

## 2026-10-09 persisted metadata migration

Legacy process argument values in current and historical graph metadata are removed by schema 6, including nested argv/argv_json fields. Argument counts and unrelated safe metadata remain available. Each migration and version marker is transactional; a deliberately incomplete v5 fixture verifies rollback rather than partial cleanup. Twin's own SQLite connections enable secure_delete. This establishes logical cleanup only, without a claim about filesystem forensic erasure, WAL files, backups or old exports. Run `twin init` to upgrade existing stores.

Process incarnation follow-up: known start ticks and boot identities survive temporarily missing reads with explicit identity-known flags; they are retained for comparison, not new observation claims. Changed identities or gone-process reappearance close active and stale touching edges, clear previous-incarnation observation links, and reset evidence/lifetime counters. New process lifetimes never inherit an old start identity when the current read is missing. Fixture regressions cover known → unknown → changed start/boot, stale eBPF evidence, and same-identity gone reappearance.

## 2026-10-09 specialized neighborhood batching

Service, file, parent-process, container, library, mount and directory views load peer nodes in bounded batches. Service/file/parent proof rendering also batches edge links and observation rows; endpoint views batch their previously individual edge-link reads. The temporary proof map exists only for that neighborhood. Library package facts and mount service discovery batch incoming edges through `Store::list_active_edges_to_nodes`, using parameter-bound chunks of 500 and deduplicated IDs. Output ordering, full labels, observation references and historical graph selection are unchanged; malformed observation rows retain the prior specialized-view omission behavior.

Verification: app scan_graph (47), nginx_config (11), temporal (13), mounts (3), libraries (6), containers (7), graph_batch (1), store graph_typed (7), and CLI output (63) tests passed. The new app fixture crosses a SQL chunk boundary with 501 peers and individual proof links, including one malformed observation; the store fixture compares batch results with individual reads for duplicate, missing and stale targets.

An isolated synthetic SQLite benchmark measured complete service graph reads with one cgroup proof per owned process, one warm-up and seven timed reads, using debug builds. Median latency before/after: 500 peers 24.667/9.220 ms; 1500 peers 70.561/25.599 ms; 3000 peers 146.831/51.999 ms. This is a roughly 2.7–2.8x improvement for that fixture, not a host-wide throughput or memory claim. No persistent cache or historical-row substitution is introduced.
