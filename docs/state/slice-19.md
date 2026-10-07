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
