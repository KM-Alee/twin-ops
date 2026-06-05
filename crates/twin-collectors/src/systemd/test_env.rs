pub fn should_skip_live_dbus() -> bool {
    std::env::var("TWIN_SYSTEMD_UNIT_ROOT").is_ok()
        || std::env::var("TWIN_SYSTEMD_CGROUP_MAP").is_ok()
}
