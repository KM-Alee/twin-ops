# Slice 8.5 Implementation Plan: Declared Dependencies and Honest Detection

**Prerequisite:** slice 8 complete (`twin impact` MVP with risk/evidence/unknowns).  
**Motivation:** Live desktop testing showed `service-depends-on: 0`, hundreds of unmapped sockets under unprivileged scan, and **unknown** risk on most services because (a) twin only infers `depends_on` from mappable TCP, and (b) impact treats **global** scan gaps as per-target uncertainty.

**Implemented layout:** follow `docs/code-layout.md`. This slice extends collectors, scan graph build, graph/impact traversal, and doctor — no new top-level crates unless `twin-collectors` systemd module justifies it.

See `docs/state/slice-08.5.md` for implementation status.
