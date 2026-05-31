use twin_core::config::{RetentionConfig, TwinConfig, DEFAULT_CONFIG_TOML};

#[test]
fn default_serializes_to_valid_toml() {
    let toml_str = toml::to_string_pretty(&TwinConfig::default()).expect("serialize");
    assert!(toml::from_str::<TwinConfig>(&toml_str).is_ok());
    assert!(toml_str.contains("raw_observations_days = 7"));
}

#[test]
fn deserializes_from_toml() {
    let input = r#"
[retention]
raw_observations_days = 14
graph_history_days = 60
ebpf_events_days = 5
"#;
    let cfg: TwinConfig = toml::from_str(input).expect("parse");
    assert_eq!(cfg.retention.raw_observations_days, 14);
    assert_eq!(cfg.retention.graph_history_days, 60);
    assert_eq!(cfg.retention.ebpf_events_days, 5);
}

#[test]
fn roundtrip_default() {
    let original = TwinConfig::default();
    let toml_str = toml::to_string_pretty(&original).expect("serialize");
    let parsed: TwinConfig = toml::from_str(&toml_str).expect("deserialize");
    assert_eq!(parsed, original);
}

#[test]
fn missing_fields_get_defaults() {
    let input = "[retention]\n";
    let cfg: TwinConfig = toml::from_str(input).expect("parse");
    assert_eq!(cfg.retention, RetentionConfig::default());
}

#[test]
fn shipped_template_matches_default_values() {
    let cfg: TwinConfig = toml::from_str(DEFAULT_CONFIG_TOML).expect("parse template");
    assert_eq!(cfg, TwinConfig::default());
}
