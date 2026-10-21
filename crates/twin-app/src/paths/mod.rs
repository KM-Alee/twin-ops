mod host;
mod layout;
mod resolve;
mod xdg;

pub use host::{
    default_proc_root, host_dbus_socket, host_path, host_root, is_host_proc_root, logical_path,
};
pub use layout::TwinLayout;
pub use resolve::{resolve_command_paths, CommandPaths};
