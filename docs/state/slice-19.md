# Slice 19: Config Files, nginx Parser, and Config Impact

## Status: complete

## Implemented

- Crate `twin-config`. It parses nginx text for `proxy_pass` and fingerprints file bytes. It does not read the host. `twin-app` reads the discovered file and passes the bytes in.
- Scan fingerprints each discovered config file into the file node's `content_hash` metadata. A later scan with different bytes records a node change, so `twin what-changed` lists the file.
- `proxy_pass http(s)://ip:port` becomes a `file REFERENCES port` edge and, when a service already `listens_on` that port, a `service PROXIES_TO service` edge. Unix targets become a file reference to the socket. A hostname that is not an address is a warning.
- Malformed `proxy_pass` lines are `config_parse` warnings on the scan result. The scan still completes.
- `twin graph` shows `proxies to` separately from runtime `depends on`, with evidence `proxy_pass …; <listener> listens on <port>`. A file neighborhood lists `references`.
- `twin emulate delete` on a config file adds a `proxies_to` evidence line for each proxy the configured service would lose. The file is not deleted.

## Decisions

- `CONFIGURED_BY` discovery is unchanged. Proxy parsing runs after that discovery, only for files the scan already found.
- The proxy target must be an IP or a unix path. Resolving hostnames would invent a listener.
- `PROXIES_TO` is inferred and only created when an active `listens_on` edge already names the target service. The file `REFERENCES` the port or socket either way.
- `http://` and `https://` values in observation metadata are redacted as connection strings. The scheme is stored beside the redacted value, and graph evidence rebuilds `scheme://endpoint` from that. The raw URL is not copied into `raw_ref`.
- Evidence strength for the proxy line is moderate, matching config-only evidence.

## Deviations from Plan

- The parser recognizes a `proxy_pass` directive at the start of a line. A directive buried in the same line as other nginx syntax is ignored.
- Hostname `proxy_pass` targets warn. They do not create a service edge.

## Acceptance Checklist

- [x] nginx proxy relationships are inferred
- [x] malformed configs warn, not crash
- [x] config file changes appear in `what-changed`
- [x] delete-config emulation names the proxy that would be lost
- [x] `cargo fmt --check`
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`

## Verification

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## 2026-10-08 follow-up: Nginx statement parsing

The parser recognizes inline, multiline, and quoted proxy_pass directives while respecting comments and quoted separators. Missing terminators and malformed targets warn. Tests cover formatting, quotes, comments, and malformed statements. Include expansion and hostname/upstream resolution remain unimplemented; no DNS lookup is performed.

## Additional 2026-10-08 follow-up

Include discovery follows relative paths from the nginx prefix and absolute paths, expands * and ?, deduplicates canonical paths to cap cycles, and reports missing/unreadable includes. Expansion is bounded to 512 files/paths and each config read to 1 MiB. Bracket glob patterns and unresolved variables are reported as unsupported. Included files receive configured_by/fingerprint/reference evidence. Proxy evidence stores only sanitized origins/socket paths: URL userinfo, request paths, fragments, and query values are discarded. Tests cover cyclic includes, glob discovery, missing files, unchanged source bytes, secret-safe evidence, IPv6 default ports, and http://unix targets. Hostname and named-upstream resolution remain unimplemented.


## 2026-10-08 follow-up: named upstream groups

Concrete server backends in named upstream blocks now resolve proxy_pass group references across discovered included files belonging to the same service. IPv4, IPv6 and Unix targets are recognized; backup candidates are retained, down servers excluded. Duplicate definitions are ambiguous and produce warnings instead of arbitrarily choosing a backend. TCP relationships retain references to both proxy and upstream definition files, with file-specific raw evidence references. Configuration still establishes inferred relationships only when an observed listener identifies the target service. Tests cover cross-file groups, down/backup servers, duplicate groups, both address families and secret-safe malformed values.

DNS backend hostnames remain unresolved. Includes embedded inside an upstream block do not inherit that parsing context. HTTP and stream contexts are not distinct group namespaces, so duplicate names are conservatively ambiguous. Unix backends retain file references; runtime Unix listener proxy attribution remains limited.


## 2026-10-08 continuation: configuration uncertainty and upstream validation

Nginx proxy/reference edges now name their nginx source explicitly; legacy proxy metadata is recognized during finalization. Incomplete quotes/blocks, unmatched closures, failed includes, unreadable files, malformed directives and ambiguous upstreams mark unseen nginx relationships stale rather than gone. Unseen endpoints supported by those relationships also become stale, while independently re-observed sockets stay active. Last-seen timestamps are not refreshed by missing evidence. A later complete scan can confirm removal, and re-observation reactivates the relationship. Uncertainty is conservatively scoped to the nginx source for the scan, not to individual service/file declarations.

Only nginx service configuration bytes are interpreted as nginx; systemd files and other service configs are still fingerprinted without nginx parsing. Physical systemd fixture paths are read directly rather than accidentally remapped under the fake host root. Named upstream lookup is case-insensitive. A group reference with an explicit port is rejected and warned, matching nginx upstream validation rather than ignoring that port. Parser and app tests cover these cases and a full active → stale → gone → active lifecycle.


Continuation verification passed: `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`. Full logs: `/tmp/twin-continuation-*.log`.

## 2026-10-09 contextual includes, transport and DNS

Nginx include expansion now substitutes bounded include text in its enclosing context before parsing. Limits: 512 reads, 32 nested paths, 8 MiB aggregate text and 1 MiB/file; cycles and unreadable includes warn and preserve stale uncertainty. File fingerprints/configured-by links remain individually collected. HTTP and stream groups with the same name resolve independently, including UDP stream listeners whose proxy comes before the listen directive. Unsupported stream URL schemes are rejected. Declared Unix upstream paths now join separately observed local service listeners.

Hostname targets use the existing bounded SystemDnsReader, injected through scan_in_with_dns for deterministic tests. Resolver errors are sanitized and suppress unresolved inference. Current DNS-backed references are Inferred and supported by typed system_dns/dns_address_resolved observations; they do not prove nginx's own resolver answer or historical hostname ownership. IPv6 normalization strips display brackets before typed endpoint construction. Config origins stay sanitized; URL credentials/path/query values never enter evidence.

`twin-config` now depends inward on twin-core to share TransportProtocol rather than invent a second TCP/UDP vocabulary. Parser regressions cover context/ranges/group separation and UDP order. App regressions cover included upstream servers, both source files, DNS IPv4/IPv6 provenance and UDP-versus-TCP references. Runtime hostname answers can differ across nginx namespaces/resolver configuration; no DNS answer alone proves backend health or a runtime connection.
