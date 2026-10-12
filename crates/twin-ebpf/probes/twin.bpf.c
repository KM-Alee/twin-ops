typedef unsigned int u32;
typedef unsigned short u16;
typedef unsigned long long u64;
typedef long long s64;
#define SEC(name) __attribute__((section(name), used))
#define RING(name)                                                             \
  struct {                                                                     \
    int map_type;                                                              \
    int key_size;                                                              \
    int value_size;                                                            \
    int max_entries;                                                           \
    int map_flags;                                                             \
  } name SEC("maps") = {27, 0, 0, 1048576, 0}
#define MAP(name, type, key, value, count)                                     \
  struct {                                                                     \
    int map_type;                                                              \
    int key_size;                                                              \
    int value_size;                                                            \
    int max_entries;                                                           \
    int map_flags;                                                             \
  } name SEC("maps") = {type, sizeof(key), sizeof(value), count, 0}
static u64 (*pid_tgid)(void) = (void *)14;
static u64 (*uid_gid)(void) = (void *)15;
static u64 (*ktime)(void) = (void *)5;
static u64 (*cgroup)(void) = (void *)80;
static void *(*task)(void) = (void *)35;
static long (*read_kernel)(void *, u32, const void *) = (void *)113;
static long (*comm)(void *, u32) = (void *)16;
static void *(*lookup)(void *, const void *) = (void *)1;
static long (*update)(void *, const void *, const void *, u64) = (void *)2;
static long (*remove_key)(void *, const void *) = (void *)3;
static long (*output)(void *, const void *, u64, u64) = (void *)130;
struct pidns_info {
  u32 pid, tgid;
};
static long (*visible_pid)(u64, u64, struct pidns_info *, u32) = (void *)120;
struct ns_common {
  u32 inum;
} __attribute__((preserve_access_index));
struct net {
  struct ns_common ns;
} __attribute__((preserve_access_index));
struct possible_net_t {
  struct net *net;
} __attribute__((preserve_access_index));
struct in6_addr {
  unsigned char bytes[16];
} __attribute__((preserve_access_index));
struct sock_common {
  u32 skc_daddr, skc_rcv_saddr;
  u16 skc_dport, skc_num, skc_family;
  struct possible_net_t skc_net;
  struct in6_addr skc_v6_daddr, skc_v6_rcv_saddr;
} __attribute__((preserve_access_index));
struct sock {
  struct sock_common __sk_common;
  u16 sk_protocol, sk_type;
} __attribute__((preserve_access_index));
struct nsproxy {
  struct net *net_ns;
} __attribute__((preserve_access_index));
struct inode {
  u16 i_mode;
} __attribute__((preserve_access_index));
struct file {
  struct inode *f_inode;
  void *private_data;
} __attribute__((preserve_access_index));
struct fdtable {
  u32 max_fds;
  struct file **fd;
} __attribute__((preserve_access_index));
struct files_struct {
  struct fdtable *fdt;
} __attribute__((preserve_access_index));
struct socket {
  struct file *file;
  struct sock *sk;
} __attribute__((preserve_access_index));
struct task_struct {
  struct task_struct *real_parent;
  struct task_struct *group_leader;
  int tgid;
  u64 start_boottime;
  struct nsproxy *nsproxy;
  struct files_struct *files;
} __attribute__((preserve_access_index));
struct exec_event {
  u32 pid, ppid;
  u64 cgroup, timestamp;
  char comm[16];
  u32 uid;
  u64 namespace, start_ticks;
} __attribute__((packed));
struct tcp_event {
  u32 pid;
  u64 cgroup, timestamp;
  u16 port;
  unsigned char family, reserved, source[16], destination[16];
  u64 namespace, start_ticks;
} __attribute__((packed));
struct accept_event {
  u32 pid;
  u64 cgroup, timestamp;
  u16 sport, dport;
  unsigned char family, reserved, source[16], destination[16];
  u64 namespace, start_ticks;
} __attribute__((packed));
struct syscall_enter {
  u64 common, number, fd;
};
struct syscall_exit {
  u64 common, number;
  s64 result;
};
struct socket_pending {
  struct tcp_event origin;
  u32 fd, kind;
  u64 file;
};
struct state_context {
  u64 common, socket;
  u32 oldstate, newstate;
  u16 sport, dport, family, protocol;
  unsigned char source[4], destination[4], source6[16], destination6[16];
};
RING(EVENTS);
RING(TCP_EVENTS);
MAP(EXEC_DROPS, 2, u32, u64, 1);
MAP(TCP_DROPS, 2, u32, u64, 1);
MAP(PID_NAMESPACE, 2, u32, u64[2], 1);
MAP(PENDING, 1, u64, u64, 16384);
MAP(ORIGINS, 1, u64, struct tcp_event, 16384);
MAP(SOCKET_PENDING, 1, u64, struct socket_pending, 16384);
static __attribute__((always_inline)) void dropped(void *map) {
  u32 key = 0;
  u64 *count = lookup(map, &key);
  if (count)
    __sync_fetch_and_add(count, 1);
}
static __attribute__((always_inline)) u32 scanner_pid(void) {
  u32 key = 0;
  u64 *configuration = lookup(&PID_NAMESPACE, &key);
  struct pidns_info identity = {};
  if (!configuration || visible_pid(configuration[0], configuration[1],
                                    &identity, sizeof(identity)) < 0)
    return 0;
  return identity.tgid;
}
SEC("tracepoint/sched/sched_process_exec") int exec(void *ctx) {
  struct exec_event event = {};
  struct task_struct *current = task(), *parent = 0;
  int parent_pid = 0;
  event.pid = scanner_pid();
  if (!event.pid) {
    dropped(&EXEC_DROPS);
    return 0;
  }
  event.uid = uid_gid();
  event.cgroup = cgroup();
  event.timestamp = ktime();
  if (read_kernel(&parent, sizeof(parent), &current->real_parent) < 0 ||
      !parent ||
      read_kernel(&parent_pid, sizeof(parent_pid), &parent->tgid) < 0) {
    dropped(&EXEC_DROPS);
    return 0;
  }
  struct task_struct *leader = 0;
  struct nsproxy *proxy = 0;
  struct net *net = 0;
  u32 namespace = 0;
  u64 start = 0;
  if (read_kernel(&leader, sizeof(leader), &current->group_leader) < 0 ||
      !leader ||
      read_kernel(&start, sizeof(start), &leader->start_boottime) < 0 ||
      read_kernel(&proxy, sizeof(proxy), &current->nsproxy) < 0 || !proxy ||
      read_kernel(&net, sizeof(net), &proxy->net_ns) < 0 || !net ||
      read_kernel(&namespace, sizeof(namespace), &net->ns.inum) < 0 ||
      !namespace) {
    dropped(&EXEC_DROPS);
    return 0;
  }
  event.namespace = namespace;
  event.start_ticks = start / 10000000;
  u32 key = 0;
  u64 *configuration = lookup(&PID_NAMESPACE, &key);
  if (configuration && configuration[1] == 0xeffffffcULL)
    event.ppid = parent_pid;
  if (comm(event.comm, sizeof(event.comm)) < 0 ||
      output(&EVENTS, &event, sizeof(event), 0) < 0)
    dropped(&EXEC_DROPS);
  return 0;
}
static __attribute__((always_inline)) struct file *descriptor_file(u32 fd) {
  struct task_struct *current = task();
  struct files_struct *files = 0;
  struct fdtable *table = 0;
  struct file **descriptors = 0, *file = 0;
  u32 maximum = 0;
  if (read_kernel(&files, sizeof(files), &current->files) < 0 || !files ||
      read_kernel(&table, sizeof(table), &files->fdt) < 0 || !table ||
      read_kernel(&maximum, sizeof(maximum), &table->max_fds) < 0 || fd >= maximum ||
      read_kernel(&descriptors, sizeof(descriptors), &table->fd) < 0 || !descriptors ||
      read_kernel(&file, sizeof(file), &descriptors[fd]) < 0)
    return 0;
  return file;
}
static __attribute__((always_inline)) int socket_enter(struct syscall_enter *ctx, u32 kind) {
  u64 id = pid_tgid(), start = 0;
  struct socket_pending pending = {};
  struct task_struct *current = task(), *leader = 0;
  remove_key(&SOCKET_PENDING, &id);
  pending.origin.pid = scanner_pid();
  if (!pending.origin.pid ||
      read_kernel(&leader, sizeof(leader), &current->group_leader) < 0 ||
      !leader || read_kernel(&start, sizeof(start), &leader->start_boottime) < 0) {
    dropped(&TCP_DROPS);
    return 0;
  }
  pending.origin.cgroup = cgroup();
  pending.origin.start_ticks = start / 10000000;
  pending.fd = ctx->fd;
  pending.kind = kind;
  if (kind == 2)
    pending.file = (u64)descriptor_file(pending.fd);
  if (update(&SOCKET_PENDING, &id, &pending, 0) < 0)
    dropped(&TCP_DROPS);
  return 0;
}
// Protocol/type became bitfields on some kernels; CO-RE extracts their actual width.
#define READ_SOCK_FIELD(destination, socket, field)                             \
  ({                                                                            \
    u64 value = 0;                                                              \
    long result = read_kernel(&value,                                            \
      __builtin_preserve_field_info((socket)->field, 0),                         \
      (const char *)(socket) + __builtin_preserve_field_info((socket)->field, 1));\
    value <<= __builtin_preserve_field_info((socket)->field, 4);                  \
    value >>= __builtin_preserve_field_info((socket)->field, 5);                  \
    *(destination) = value;                                                     \
    result;                                                                     \
  })
static __attribute__((always_inline)) int socket_read(u32 fd, u64 expected_file, struct tcp_event *event, u16 *remote_port) {
  struct file *file = descriptor_file(fd), *socket_file = 0;
  struct inode *inode = 0;
  struct socket *socket = 0;
  struct sock *sk = 0;
  struct net *net = 0;
  u32 namespace = 0;
  u16 mode = 0, family = 0, protocol = 0, type = 0, peer = 0;
  if (!file || (expected_file && (u64)file != expected_file) ||
      read_kernel(&inode, sizeof(inode), &file->f_inode) < 0 || !inode ||
      read_kernel(&mode, sizeof(mode), &inode->i_mode) < 0)
    return -1;
  if ((mode & 0170000) != 0140000)
    return -1;
  if (read_kernel(&socket, sizeof(socket), &file->private_data) < 0 || !socket ||
      read_kernel(&socket_file, sizeof(socket_file), &socket->file) < 0 || socket_file != file ||
      read_kernel(&sk, sizeof(sk), &socket->sk) < 0 || !sk ||
      read_kernel(&family, sizeof(family), &sk->__sk_common.skc_family) < 0 ||
      READ_SOCK_FIELD(&protocol, sk, sk_protocol) < 0 ||
      READ_SOCK_FIELD(&type, sk, sk_type) < 0)
    return -1;
  if ((family != 2 && family != 10) || protocol != 6 || type != 1)
    return 0;
  if (read_kernel(&net, sizeof(net), &sk->__sk_common.skc_net.net) < 0 || !net ||
      read_kernel(&namespace, sizeof(namespace), &net->ns.inum) < 0 || !namespace ||
      read_kernel(&event->port, sizeof(event->port), &sk->__sk_common.skc_num) < 0 ||
      read_kernel(&peer, sizeof(peer), &sk->__sk_common.skc_dport) < 0)
    return -1;
  event->namespace = namespace;
  event->family = family == 2 ? 4 : 6;
  *remote_port = __builtin_bswap16(peer);
  if (family == 2) {
    if (read_kernel(event->source, 4, &sk->__sk_common.skc_rcv_saddr) < 0 ||
        read_kernel(event->destination, 4, &sk->__sk_common.skc_daddr) < 0)
      return -1;
  } else {
    if (read_kernel(event->source, 16, &sk->__sk_common.skc_v6_rcv_saddr) < 0 ||
        read_kernel(event->destination, 16, &sk->__sk_common.skc_v6_daddr) < 0)
      return -1;
  }
  if (descriptor_file(fd) != file)
    return -1;
  return 1;
}
static __attribute__((always_inline)) int socket_exit(struct syscall_exit *ctx, u32 kind) {
  u64 id = pid_tgid();
  struct socket_pending *stored = lookup(&SOCKET_PENDING, &id);
  if (!stored)
    return 0;
  struct socket_pending pending = *stored;
  remove_key(&SOCKET_PENDING, &id);
  if (pending.kind != kind || ctx->result < 0 || (kind == 2 && ctx->result != 0))
    return 0;
  u16 remote_port = 0;
  u32 fd = kind == 1 ? ctx->result : pending.fd;
  if (kind == 1 && ctx->result > 0x7fffffff)
    return 0;
  if (kind == 2 && !pending.file) {
    dropped(&TCP_DROPS);
    return 0;
  }
  int read = socket_read(fd, pending.file, &pending.origin, &remote_port);
  if (read < 0) {
    dropped(&TCP_DROPS);
    return 0;
  }
  if (!read)
    return 0;
  pending.origin.timestamp = ktime();
  if (kind == 1) {
    struct accept_event event = {};
    event.pid = pending.origin.pid;
    event.cgroup = pending.origin.cgroup;
    event.timestamp = pending.origin.timestamp;
    event.sport = pending.origin.port;
    event.dport = remote_port;
    event.family = pending.origin.family;
    event.namespace = pending.origin.namespace;
    event.start_ticks = pending.origin.start_ticks;
    __builtin_memcpy(event.source, pending.origin.source, 16);
    __builtin_memcpy(event.destination, pending.origin.destination, 16);
    if (output(&TCP_EVENTS, &event, sizeof(event), 0) < 0)
      dropped(&TCP_DROPS);
  } else {
    pending.origin.reserved = 2;
    __builtin_memset(pending.origin.destination, 0, 16);
    if (output(&TCP_EVENTS, &pending.origin, sizeof(pending.origin), 0) < 0)
      dropped(&TCP_DROPS);
  }
  return 0;
}
SEC("tracepoint/syscalls/sys_enter_accept") int accept_enter(struct syscall_enter *ctx) {
  return socket_enter(ctx, 1);
}
SEC("tracepoint/syscalls/sys_enter_accept4") int accept4_enter(struct syscall_enter *ctx) {
  return socket_enter(ctx, 1);
}
SEC("tracepoint/syscalls/sys_exit_accept") int accept_exit(struct syscall_exit *ctx) {
  return socket_exit(ctx, 1);
}
SEC("tracepoint/syscalls/sys_exit_accept4") int accept4_exit(struct syscall_exit *ctx) {
  return socket_exit(ctx, 1);
}
SEC("tracepoint/syscalls/sys_enter_bind") int bind_enter(struct syscall_enter *ctx) {
  return socket_enter(ctx, 2);
}
SEC("tracepoint/syscalls/sys_exit_bind") int bind_exit(struct syscall_exit *ctx) {
  return socket_exit(ctx, 2);
}
SEC("tracepoint/syscalls/sys_enter_connect") int connect_enter(void *ctx) {
  u64 id = pid_tgid(), value = cgroup();
  if (update(&PENDING, &id, &value, 0) < 0)
    dropped(&TCP_DROPS);
  return 0;
}
SEC("tracepoint/syscalls/sys_exit_connect") int connect_exit(void *ctx) {
  u64 id = pid_tgid();
  remove_key(&PENDING, &id);
  return 0;
}
SEC("tracepoint/sched/sched_process_exit") int connect_cleanup(void *ctx) {
  u64 id = pid_tgid();
  remove_key(&PENDING, &id);
  remove_key(&SOCKET_PENDING, &id);
  return 0;
}
SEC("tracepoint/sock/inet_sock_set_state") int tcp(struct state_context *ctx) {
  if (ctx->protocol != 6 || (ctx->family != 2 && ctx->family != 10))
    return 0;
  u64 socket = ctx->socket;
  if (ctx->newstate == 2 && ctx->oldstate == 7) {
    u64 id = pid_tgid();
    u64 *owner_cgroup = lookup(&PENDING, &id);
    if (!owner_cgroup)
      return 0;
    struct tcp_event event = {};
    struct task_struct *current = task(), *leader = 0;
    struct sock *sk = (void *)ctx->socket;
    struct net *net = 0;
    u32 namespace = 0;
    u64 start = 0;
    if (read_kernel(&net, sizeof(net), &sk->__sk_common.skc_net.net) < 0 ||
        !net || read_kernel(&namespace, sizeof(namespace), &net->ns.inum) < 0 ||
        read_kernel(&leader, sizeof(leader), &current->group_leader) < 0 ||
        !leader ||
        read_kernel(&start, sizeof(start), &leader->start_boottime) < 0 ||
        !namespace) {
      dropped(&TCP_DROPS);
      return 0;
    }
    event.namespace = namespace;
    event.start_ticks = start / 10000000;
    event.pid = scanner_pid();
    if (!event.pid) {
      dropped(&TCP_DROPS);
      return 0;
    }
    event.cgroup = *owner_cgroup;
    event.family = ctx->family == 2 ? 4 : 6;
    event.port = ctx->dport;
    if (ctx->family == 2) {
      __builtin_memcpy(event.source, ctx->source, 4);
      __builtin_memcpy(event.destination, ctx->destination, 4);
    } else {
      __builtin_memcpy(event.source, ctx->source6, 16);
      __builtin_memcpy(event.destination, ctx->destination6, 16);
    }
    if (update(&ORIGINS, &socket, &event, 0) < 0)
      dropped(&TCP_DROPS);
  } else if (ctx->newstate == 1) {
    struct tcp_event *origin = lookup(&ORIGINS, &socket);
    if (origin) {
      origin->timestamp = ktime();
      if (output(&TCP_EVENTS, origin, sizeof(*origin), 0) < 0)
        dropped(&TCP_DROPS);
      remove_key(&ORIGINS, &socket);
    }
  } else if (ctx->newstate == 7)
    remove_key(&ORIGINS, &socket);
  return 0;
}
char LICENSE[] SEC("license") = "GPL";
