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

#[test]
fn parses_inline_multiline_and_quoted_directives_without_matching_values() {
    let parsed = parse_nginx(
        r#"
server { location / { proxy_pass "http://127.0.0.1:8000/path#fragment"; } proxy_pass
 https://[::1]:8443;
 set $example "proxy_pass http://ignored:80;";
 # proxy_pass http://ignored:81;
 proxy_pass 'http://127.0.0.1:9000'; }
"#,
    );
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(parsed.proxies.len(), 3);
    assert_eq!(
        parsed.proxies[1].target,
        ProxyTarget::Tcp {
            host: "::1".into(),
            port: 8443
        }
    );
}

#[test]
fn missing_semicolon_before_block_end_warns() {
    let parsed = parse_nginx("server { proxy_pass http://127.0.0.1:8000 }");
    assert!(parsed.proxies.is_empty());
    assert_eq!(parsed.warnings.len(), 1);
}

#[test]
fn include_directives_and_proxy_origins_do_not_retain_url_credentials_or_query_values() {
    let parsed = parse_nginx(
        r#"include "conf.d/*.conf"; include /etc/nginx/sites-enabled/*;
proxy_pass https://user:PRIVATE_PASSWORD@127.0.0.1:8443/path?token=PRIVATE_TOKEN;
proxy_pass http://[::1];
proxy_pass http://unix:/run/app.sock:/private/path;
"#,
    );
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(
        parsed.includes,
        vec!["conf.d/*.conf", "/etc/nginx/sites-enabled/*"]
    );
    assert_eq!(parsed.proxies[0].raw, "https://127.0.0.1:8443");
    assert_eq!(parsed.proxies[1].raw, "http://[::1]:80");
    assert_eq!(
        parsed.proxies[2].target,
        ProxyTarget::Unix {
            path: "/run/app.sock".into()
        }
    );
    assert!(!format!("{parsed:?}").contains("PRIVATE_"));
}

#[test]
fn upstream_blocks_keep_only_safe_backend_addresses_and_reject_duplicates() {
    let parsed = parse_nginx(
        r#"
http {
 upstream api { server 127.0.0.1:8000 weight=2; server [::1]:8001 backup; server 192.0.2.1:8002 down; server unix:/run/api.sock; }
 server { location / { proxy_pass http://api/private?token=SECRET; } }
 upstream duplicate { server 127.0.0.1:9000; }
 upstream duplicate { server 127.0.0.1:9001; }
 upstream bad { server user:SECRET@host:80; }
}
"#,
    );
    assert_eq!(parsed.upstreams["api"].len(), 3);
    assert!(
        matches!(&parsed.upstreams["api"][0], ProxyTarget::Tcp { host, port } if host == "127.0.0.1" && *port == 8000)
    );
    assert!(!parsed.upstreams.contains_key("duplicate"));
    assert!(parsed.upstreams["bad"].is_empty());
    assert!(parsed.warnings.len() >= 2);
    assert!(!format!("{parsed:?}").contains("SECRET"));
}

#[test]
fn incomplete_blocks_quotes_and_unmatched_closures_warn() {
    for config in ["http {", "http { proxy_pass 'http://127.0.0.1:80;", "}"] {
        assert!(
            !twin_config::parse_nginx(config).warnings.is_empty(),
            "{config}"
        );
    }
}

#[test]
fn proxy_targets_retain_explicit_port_presence_without_credentials() {
    let parsed = twin_config::parse_nginx("proxy_pass http://pool; proxy_pass http://pool:80; proxy_pass https://user:SECRET@[::1]:443/private;");
    assert!(parsed.warnings.is_empty());
    assert!(!parsed.proxies[0].has_explicit_port);
    assert!(parsed.proxies[1].has_explicit_port);
    assert!(parsed.proxies[2].has_explicit_port);
    assert!(!format!("{parsed:?}").contains("SECRET"));
}

#[test]
fn http_and_stream_upstreams_with_same_name_are_independent() {
    let parsed = parse_nginx("http { upstream backend { server 127.0.0.1:8000; } server { proxy_pass http://backend; } } stream { upstream backend { server 127.0.0.1:9000; } server { proxy_pass backend; } }");
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(
        parsed.upstreams["backend"],
        vec![ProxyTarget::Tcp {
            host: "127.0.0.1".into(),
            port: 8000
        }]
    );
    assert_eq!(
        parsed.upstreams["stream:backend"],
        vec![ProxyTarget::Tcp {
            host: "127.0.0.1".into(),
            port: 9000
        }]
    );
    assert!(!parsed.proxies[0].is_stream);
    assert!(parsed.proxies[1].is_stream);
}

#[test]
fn include_ranges_preserve_context_and_ignore_quoted_comment_like_values() {
    let text = "http { upstream api { # ignored include bad;\n include 'backends.conf'; } set $x 'include fake;'; }";
    let parsed = parse_nginx(text);
    assert_eq!(parsed.includes, ["backends.conf"]);
    assert_eq!(
        &text[parsed.include_ranges[0].clone()],
        "include 'backends.conf';"
    );
}

#[test]
fn stream_concrete_tcp_and_unix_targets_are_supported() {
    let parsed = parse_nginx("stream { server { proxy_pass 127.0.0.1:9000; } server { proxy_pass unix:/run/app.sock; } }");
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert!(matches!(
        parsed.proxies[0].target,
        ProxyTarget::Tcp { port: 9000, .. }
    ));
    assert!(parsed.proxies.iter().all(|proxy| proxy.is_stream));
}

#[test]
fn stream_udp_listen_qualifies_only_its_own_server_even_after_proxy_pass() {
    let parsed = parse_nginx("stream { server { proxy_pass 127.0.0.1:53; listen 5300 udp; } server { listen 5301; proxy_pass 127.0.0.1:53; } }");
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    assert_eq!(
        parsed.proxies[0].protocol,
        twin_core::TransportProtocol::Udp
    );
    assert_eq!(
        parsed.proxies[1].protocol,
        twin_core::TransportProtocol::Tcp
    );
}
