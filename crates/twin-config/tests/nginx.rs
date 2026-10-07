use twin_config::{content_fingerprint, parse_nginx, ProxyTarget};

#[test]
fn parses_proxy_pass_and_ignores_comments() {
    let text = r#"
events {}
http {
  server {
    # proxy_pass http://ignored.example;
    location / {
      proxy_pass http://127.0.0.1:8000/app;
    }
    location /sock {
      proxy_pass unix:/run/app.sock;
    }
  }
}
"#;
    let parsed = parse_nginx(text);
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.proxies.len(), 2);
    assert_eq!(
        parsed.proxies[0].target,
        ProxyTarget::Tcp {
            host: "127.0.0.1".to_string(),
            port: 8000,
        }
    );
    assert_eq!(
        parsed.proxies[1].target,
        ProxyTarget::Unix {
            path: "/run/app.sock".to_string(),
        }
    );
}

#[test]
fn malformed_proxy_pass_warns() {
    let parsed = parse_nginx("proxy_pass ;\nproxy_pass http://127.0.0.1:notaport;\n");
    assert!(parsed.proxies.is_empty());
    assert_eq!(parsed.warnings.len(), 2);
}

#[test]
fn fingerprint_changes_when_bytes_change() {
    assert_ne!(
        content_fingerprint(b"proxy_pass http://127.0.0.1:8000;"),
        content_fingerprint(b"proxy_pass http://127.0.0.1:8001;")
    );
}
