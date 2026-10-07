pub const STARTER_YAML: &str = r#"version: 1

name: local-readiness

defaults:
  min_evidence: moderate

checks:
  - name: postgres is listening
    assert:
      node: port:tcp:127.0.0.1:5432
      exists: true

  - name: postgres port exists
    assert:
      port: port:tcp:127.0.0.1:5432
      exists: true

  - name: django service exists
    assert:
      service: service:django.service
      exists: true

  - name: django depends on postgres
    assert:
      dependency:
        from: service:django.service
        to: service:postgresql.service
      min_evidence: moderate
"#;
