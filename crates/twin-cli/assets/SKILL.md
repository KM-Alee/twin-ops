---
name: twin
description: Read-only dependency map and blast-radius predictor for the Linux host you are operating on. Use before restarting/stopping/killing services or containers, deleting or editing system files, upgrading/removing packages, changing firewall rules or rebooting; when orienting on an unfamiliar machine; and when investigating what broke or changed. Replaces chains of systemctl/ss/lsof/docker/journalctl calls with one call.
---

# twin — know what breaks before you touch it

`twin` keeps a read-only graph of this host: services, processes, listening ports, live
connections, containers, config files, packages, and what depends on what. It never changes
the host. Every answer separates observed facts from inferences and lists coverage gaps.

## The three moves

1. **Orient (1 call instead of ~10).**
   - `twin brief` shows the host: failed services, what is exposed to the network and by whom,
     the most depended-on services, running containers, and recent significant changes.
   - `twin brief <target>` covers one service, container, port (`:5432`), file, package or pid:
     state, listeners, dependencies both ways, config files, recent changes, and what a
     restart would break.

2. **Preflight every mutating command.** Before you run anything that restarts, stops,
   kills, deletes, edits, upgrades, removes, firewalls or reboots:

   ```bash
   twin preflight "sudo systemctl restart nginx"
   ```

   Exit code `0` = go, `3` = caution, `4` = stop, `1` = error. It understands sudo/env
   prefixes, `&&`/`;`/pipes, `bash -c`, redirections, systemctl, docker/podman, rm, sed -i,
   tee, package managers, kill/pkill, kubectl, ufw/iptables and reboot.
   - **stop**: do not run it. Show the user the affected list and ask first.
   - **caution**: read the reasons and follow the advice (e.g. `nginx -t`, prefer `reload`).
   - **go**: proceed.

3. **Investigate.**
   - `twin what-changed --since 1h --target <svc>` shows restarts, config edits, listener and
     package changes around a target, in chronological order.
   - `twin why <target>` shows the evidence chain behind a dependency.

Add `--json` before or after the subcommand for machine-readable output.

## Rules

- Coverage gaps (for example "N processes hidden due to permissions") mean **unknown, not
  absent**. If they matter, say so, or suggest running twin with more privileges.
- Temporal association in `what-changed` is not causation.
- `twin` predicts; it never performs restarts, deletes or upgrades. You still run the real
  command, after the user agrees when the verdict is stop.

## Setup check

If `twin` reports that its database is not initialized, run `twin init && twin scan` once.
`twin doctor` explains missing permissions or collectors.
