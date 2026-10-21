use twin_config::{
    endpoints_in_environment, endpoints_in_value, is_secret_name, mysql_master_info, parse_caddy,
    parse_compose, parse_env_file, parse_fpm_pools, parse_haproxy, parse_nginx, postgres_primary,
    redis_primary, DeclaredEndpoint, ProxyTarget,
};
use twin_core::TransportProtocol;

const SENTINEL: &str = "SENTINEL_SECRET_42";

fn origins(endpoints: &[DeclaredEndpoint]) -> Vec<String> {
    endpoints.iter().map(DeclaredEndpoint::origin).collect()
}

fn assert_no_sentinel(what: &str, debug: String) {
    assert!(!debug.contains(SENTINEL), "{what} kept a secret: {debug}");
    assert!(!debug.contains("SENTINEL"), "{what} kept a secret: {debug}");
}

#[test]
fn urls_keep_only_scheme_host_and_port() {
    let found = endpoints_in_value(
        "DATABASE_URL",
        &format!("postgres://app:{SENTINEL}@db.internal/app?sslmode=require&password={SENTINEL}"),
    );
    assert_eq!(origins(&found), ["postgres://db.internal:5432"]);
    let found = endpoints_in_value("CACHE", &format!("rediss://:{SENTINEL}@10.0.0.7:6380/2"));
    assert_eq!(origins(&found), ["rediss://10.0.0.7:6380"]);
    let found = endpoints_in_value(
        "MONGO_URL",
        &format!("mongodb://u:{SENTINEL}@m1.internal:27017,m2.internal:27018/db"),
    );
    assert_eq!(
        origins(&found),
        ["mongodb://m1.internal:27017", "mongodb://m2.internal:27018"]
    );
    let found = endpoints_in_value("API", "https://[2001:db8::1]/v1");
    assert_eq!(origins(&found), ["https://[2001:db8::1]:443"]);
    let found = endpoints_in_value("SOCKET_URL", "unix:///run/app/app.sock");
    assert_eq!(origins(&found), ["unix:/run/app/app.sock"]);
    assert_no_sentinel("url parsing", format!("{found:?}"));
}

#[test]
fn credentials_that_break_the_url_are_dropped_not_misread_as_hosts() {
    for value in [
        "postgres://dbuser:1234/secret@db.internal/app",
        "postgres://dbuser:1234?x@db.internal/app",
        "postgres://dbuser:1234#x@db.internal/app",
        "dbuser:1234/secret@db.internal:5432",
    ] {
        let found = endpoints_in_value("DATABASE_URL", value);
        assert!(
            found
                .iter()
                .all(|endpoint| !endpoint.origin().contains("dbuser")
                    && !endpoint.origin().contains("1234")),
            "{value} -> {:?}",
            origins(&found)
        );
    }
}

#[test]
fn secret_names_are_never_parsed_and_bare_values_need_an_endpoint_name() {
    for name in [
        "DB_PASSWORD",
        "API_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "STRIPE_KEY",
        "PGPASS",
        "AUTH_URL",
        "client_secret",
    ] {
        assert!(is_secret_name(name), "{name}");
        assert!(endpoints_in_value(name, "https://example.com:8443").is_empty());
    }
    for name in ["DATABASE_URL", "KEYCLOAK_URL", "REDIS_HOST", "UPSTREAM"] {
        assert!(!is_secret_name(name), "{name}");
    }
    assert!(endpoints_in_value("BACKUP_TIME", "12:30").is_empty());
    assert!(endpoints_in_value("GREETING", "db:5432").is_empty());
    assert_eq!(
        origins(&endpoints_in_value("CACHE_HOST", "redis:6379")),
        ["redis:6379"]
    );
    assert_eq!(
        origins(&endpoints_in_value(
            "KAFKA_BROKERS",
            "k1.internal:9092,k2.internal:9092"
        )),
        ["k1.internal:9092", "k2.internal:9092"]
    );
    assert_eq!(
        origins(&endpoints_in_value("ANYTHING", "localhost:8080")),
        ["localhost:8080"]
    );
}

#[test]
fn host_and_port_settings_pair_up() {
    let environment = [
        ("PGHOST", "10.0.0.5"),
        ("PG_HOST", "10.0.0.5"),
        ("PG_PORT", "5433"),
        ("PG_PASSWORD", SENTINEL),
    ];
    let found = endpoints_in_environment(environment.iter().copied());
    assert_eq!(origins(&found), ["10.0.0.5:5433"]);
    assert_eq!(found[0].key, "PG_HOST+PG_PORT");
}

#[test]
fn env_files_parse_quotes_exports_and_comments_and_hide_values_in_debug() {
    let text = format!(
        "# comment\nexport DATABASE_URL=\"postgres://u:{SENTINEL}@db.internal:5432/app\"\nSECRET='{SENTINEL}'\nREDIS_URL=redis://cache.internal:6379 # trailing\n1BAD=x\n"
    );
    let assignments = parse_env_file(&text);
    let names: Vec<_> = assignments.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["DATABASE_URL", "SECRET", "REDIS_URL"]);
    assert_eq!(assignments[0].line, 2);
    assert_eq!(assignments[2].value, "redis://cache.internal:6379");
    assert_no_sentinel("env file debug", format!("{assignments:?}"));
    let found = endpoints_in_environment(
        assignments
            .iter()
            .map(|a| (a.name.as_str(), a.value.as_str())),
    );
    assert_eq!(
        origins(&found),
        ["postgres://db.internal:5432", "redis://cache.internal:6379"]
    );
}

#[test]
fn compose_declares_dependencies_ports_and_environment_endpoints() {
    let text = format!(
        r#"
services:
  web:
    image: app
    depends_on:
      db:
        condition: service_healthy
      cache:
        condition: service_started
    links: ["queue:mq"]
    ports:
      - "8080:80"
      - "127.0.0.1:8443:443/tcp"
      - "[::1]:9000:9000"
      - "514:514/udp"
      - "3000"
      - "8000-8010:8000-8010"
      - target: 6000
        published: 16000
        host_ip: 0.0.0.0
    environment:
      DATABASE_URL: postgres://app:{SENTINEL}@db:5432/app
      DB_PASSWORD: {SENTINEL}
      CACHE_HOST: cache:6379
      WORKERS: 4
    env_file: [.env, {{path: ./web.env, required: false}}]
  worker:
    depends_on: [db]
    environment:
      - "AMQP_URL=amqp://guest:{SENTINEL}@queue"
      - PLAIN
"#
    );
    let parsed = parse_compose(&text).expect("compose");
    assert_eq!(parsed.warnings.len(), 1, "{:?}", parsed.warnings);
    let web = &parsed.services[0];
    assert_eq!(web.name, "web");
    assert_eq!(web.depends_on, ["db", "cache", "queue"]);
    let ports: Vec<_> = web
        .ports
        .iter()
        .map(|port| {
            (
                port.host_ip.clone(),
                port.host_port,
                port.container_port,
                port.protocol,
            )
        })
        .collect();
    assert_eq!(
        ports,
        [
            (None, 8080, 80, TransportProtocol::Tcp),
            (Some("127.0.0.1".into()), 8443, 443, TransportProtocol::Tcp),
            (Some("::1".into()), 9000, 9000, TransportProtocol::Tcp),
            (None, 514, 514, TransportProtocol::Udp),
            (Some("0.0.0.0".into()), 16000, 6000, TransportProtocol::Tcp),
        ]
    );
    assert_eq!(
        origins(&web.endpoints),
        ["postgres://db:5432", "cache:6379"]
    );
    assert_eq!(web.env_files, [".env", "./web.env"]);
    let worker = &parsed.services[1];
    assert_eq!(worker.depends_on, ["db"]);
    assert_eq!(origins(&worker.endpoints), ["amqp://queue:5672"]);
    assert_no_sentinel("compose parse", format!("{parsed:?}"));
}

#[test]
fn malformed_compose_reports_a_location_and_no_text() {
    let error = parse_compose(&format!("services:\n  web: [{SENTINEL}\n")).expect_err("bad yaml");
    assert!(
        error.starts_with("compose file is not valid YAML"),
        "{error}"
    );
    assert_no_sentinel("compose error", error);
    assert!(parse_compose("version: '3'\n")
        .expect("no services")
        .services
        .is_empty());
}

#[test]
fn haproxy_servers_are_read_from_backends_and_listen_sections() {
    let text = format!(
        "global\n  stats socket /run/haproxy.sock\nuserlist admins\n  user admin password {SENTINEL}\nfrontend www\n  bind :80\n  server ignored 10.9.9.9:1\nbackend api\n  balance roundrobin\n  server api1 10.0.0.11:8080 check\n  server api2 api2.internal:8080 check # comment\n  server local unix@/run/api.sock\n  server dyn ${{API_HOST}}:80\n  server noport 10.0.0.12\n  server-template web 3 web.internal:80\nlisten stats\n  server s1 127.0.0.1:9000\n"
    );
    let parsed = parse_haproxy(&text);
    assert_eq!(
        origins(&parsed.servers),
        [
            "10.0.0.11:8080",
            "api2.internal:8080",
            "unix:/run/api.sock",
            "127.0.0.1:9000"
        ]
    );
    assert_eq!(parsed.servers[0].key, "backend api server api1");
    assert_eq!(parsed.servers[0].line, 10);
    assert_eq!(parsed.warnings.len(), 3, "{:?}", parsed.warnings);
    assert_no_sentinel("haproxy", format!("{parsed:?}"));
}

#[test]
fn caddy_reverse_proxy_and_php_fastcgi_upstreams() {
    let text = format!(
        "example.com {{\n  basic_auth {{\n    admin {SENTINEL}\n  }}\n  reverse_proxy /api/* localhost:8080 http://10.0.0.2:3000\n  reverse_proxy @ws {{\n    to app.internal:9000 h2c://10.0.0.3\n    header_up Authorization {SENTINEL}\n  }}\n  reverse_proxy :4000\n  reverse_proxy unix//run/app.sock\n  reverse_proxy {{env.UPSTREAM}}\n  php_fastcgi unix//run/php/php-fpm.sock\n  php_fastcgi 127.0.0.1\n}}\n"
    );
    let parsed = parse_caddy(&text);
    assert_eq!(
        origins(&parsed.upstreams),
        [
            "localhost:8080",
            "http://10.0.0.2:3000",
            "app.internal:9000",
            "h2c://10.0.0.3:80",
            "localhost:4000",
            "unix:/run/app.sock",
            "unix:/run/php/php-fpm.sock",
            "fastcgi://127.0.0.1:9000",
        ]
    );
    assert_eq!(parsed.warnings.len(), 1, "{:?}", parsed.warnings);
    assert_no_sentinel("caddy", format!("{parsed:?}"));
}

#[test]
fn php_fpm_pools_report_their_listen_address() {
    let text = format!(
        "[global]\nerror_log = /var/log/fpm.log\n[www]\nuser = www-data\nlisten = /run/php/php8.2-fpm.sock\nenv[DB_PASSWORD] = {SENTINEL}\n[api]\nlisten = 127.0.0.1:9001 ; comment\n[legacy]\nlisten = 9002\n"
    );
    let pools = parse_fpm_pools(&text);
    assert_eq!(
        origins(&pools),
        [
            "unix:/run/php/php8.2-fpm.sock",
            "fastcgi://127.0.0.1:9001",
            "fastcgi://0.0.0.0:9002"
        ]
    );
    assert_eq!(pools[1].key, "pool api listen");
    assert_no_sentinel("php-fpm", format!("{pools:?}"));
}

#[test]
fn replication_sources_keep_only_host_and_port() {
    let primary = postgres_primary(&format!(
        "# primary_conninfo = 'host=old'\nprimary_conninfo = 'user=replicator password={SENTINEL} host=10.0.0.1 port=5433 sslmode=require'\n"
    ))
    .expect("postgres primary");
    assert_eq!(primary.origin(), "postgres://10.0.0.1:5433");
    assert_eq!(primary.line, 2);
    let socket = postgres_primary("primary_conninfo = 'host=/var/run/postgresql'\n")
        .expect("socket primary");
    assert_eq!(
        socket.target,
        ProxyTarget::Unix {
            path: "/var/run/postgresql/.s.PGSQL.5432".into()
        }
    );
    let redis = redis_primary(&format!(
        "requirepass {SENTINEL}\nmasterauth {SENTINEL}\nreplicaof 10.0.0.2 6380\n"
    ))
    .expect("redis primary");
    assert_eq!(redis.origin(), "redis://10.0.0.2:6380");
    let mysql = mysql_master_info(&format!(
        "33\nmysql-bin.000001\n4\ndb-primary.internal\nrepl\n{SENTINEL}\n3307\n60\n"
    ))
    .expect("mysql primary");
    assert_eq!(mysql.origin(), "mysql://db-primary.internal:3307");
    assert!(mysql_master_info("1\nonly\n").is_none());
    assert_no_sentinel(
        "replication",
        format!("{primary:?} {socket:?} {redis:?} {mysql:?}"),
    );
}

#[test]
fn nginx_gateway_directives_are_backends_too() {
    let parsed = parse_nginx(
        "http { server { location ~ \\.php$ { fastcgi_pass unix:/run/php/php-fpm.sock; } location /py { uwsgi_pass 127.0.0.1:3031; } location /g { grpc_pass grpc://10.0.0.4:50051; } location /f { fastcgi_pass php_pool; } } upstream php_pool { server 127.0.0.1:9000; } }",
    );
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    let raws: Vec<_> = parsed
        .proxies
        .iter()
        .map(|proxy| proxy.raw.as_str())
        .collect();
    assert_eq!(
        raws,
        [
            "unix:/run/php/php-fpm.sock",
            "uwsgi://127.0.0.1:3031",
            "grpc://10.0.0.4:50051",
            "php_pool"
        ]
    );
}
