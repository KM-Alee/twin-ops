use std::sync::Mutex;

use twin_app::paths::TwinLayout;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn from_xdg_uses_sudo_invoker_home_not_root() {
    let _guard = ENV_LOCK.lock().expect("lock");
    let real_home = dirs::home_dir().expect("home");
    let username = real_home
        .file_name()
        .and_then(|n| n.to_str())
        .expect("username");
    if username == "root" {
        return;
    }

    let prev_uid = std::env::var("SUDO_UID").ok();
    let prev_user = std::env::var("SUDO_USER").ok();
    let prev_home = std::env::var("HOME").ok();

    std::env::set_var("SUDO_UID", "1000");
    std::env::set_var("SUDO_USER", username);
    std::env::set_var("HOME", "/root");

    let layout = TwinLayout::from_xdg().expect("layout");
    assert_eq!(
        layout.db_file(),
        real_home.join(".local/share/twin/twin.db")
    );

    restore_env("SUDO_UID", prev_uid);
    restore_env("SUDO_USER", prev_user);
    restore_env("HOME", prev_home);
}

fn restore_env(key: &str, value: Option<String>) {
    match value {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}
