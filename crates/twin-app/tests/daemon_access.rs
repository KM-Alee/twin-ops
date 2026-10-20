use std::os::unix::net::UnixStream;

use twin_app::daemon::{peer_credentials, AccessPolicy, PeerCredentials};

#[test]
fn the_policy_admits_root_the_daemon_user_and_its_group_only() {
    let policy = AccessPolicy::new(0, 960);
    let peer = |uid, gid, groups: &[u32]| PeerCredentials::new(4242, uid, gid, groups.to_vec());
    assert!(policy.allows(&peer(0, 0, &[])), "root");
    assert!(policy.allows(&peer(1000, 960, &[])), "primary group");
    assert!(
        policy.allows(&peer(1000, 1000, &[27, 960])),
        "supplementary group"
    );
    assert!(!policy.allows(&peer(1000, 1000, &[27, 100])), "other user");
    assert!(!policy.allows(&peer(65534, 65534, &[])), "nobody");

    let unprivileged = AccessPolicy::new(1000, 1000);
    assert!(
        unprivileged.allows(&peer(1000, 50, &[])),
        "the daemon's own user"
    );
    assert!(!unprivileged.allows(&peer(1001, 1001, &[])));
}

#[test]
fn peer_credentials_name_the_connecting_process() {
    let (server, _client) = UnixStream::pair().expect("pair");
    let peer = peer_credentials(&server).expect("credentials");
    let own = AccessPolicy::current().expect("own identity");
    assert_eq!(peer.pid(), std::process::id() as i32);
    assert_eq!(peer.uid(), own.uid());
    assert_eq!(peer.gid(), own.gid());
    assert!(own.allows(&peer));
}

#[test]
fn credentials_of_a_closed_peer_are_still_the_ones_recorded_at_connect() {
    let (server, client) = UnixStream::pair().expect("pair");
    drop(client);
    let peer = peer_credentials(&server).expect("credentials survive the peer");
    assert_eq!(peer.pid(), std::process::id() as i32);
}
