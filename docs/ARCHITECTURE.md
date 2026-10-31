# Architecture and lifecycle

`desktop defuss UI → native HTTP bridge → defuss-containerd ← HTTP CLI`.
Only the daemon invokes Git and the pinned smolvm engine. The shared provider adapter has platform-specific prerequisite checks; the hypervisor implementations are provided by smolvm/libkrun. Direct CLI integration is intentional while the Rust SDK's documented local platform set excludes Windows.

Metadata is SQLite/WAL with synchronous FULL. App and job records are JSON in typed tables. A single-instance file lock protects the daemon database. Jobs are serialized per app; a two-worker semaphore bounds global concurrency. Credentials and files use private state directories. The management listener is IPv4 loopback, authenticated, origin-restricted and Host-validated.

Build pipeline:

1. Initialize a bare Git repository in a private temporary directory; HTTPS fetch the requested ref without submodules/hooks. Resolve an immutable commit, archive the selected tree. No checkout filters or repository scripts execute on the host.
2. Resolve exactly oven/bun:1.4.4-distroless for the host architecture. Verify platform-manifest SHA-256 and every config/layer blob; preserve the resolved base digest in the release record. TLS uses the OS trust store, without disabling verification.
3. Create a disposable machine with private-network egress blocked, source/helper read-only shares and one writable export directory. Copy source into guest-local storage, frozen install (scripts disabled), optional build, production-only dependency reinstall.
4. Serialize app files with modes and relative symlinks into a tar layer. Delete the builder VM. On the host, refuse symlink/hardlink artifact files, discard other builder-controlled output, verify layer paths/links/type/size, and assemble the OCI layout using Rust. Include a compatible Docker-save index referring to the same blobs for smolvm's archive importer. No Docker binary/daemon is involved.
5. Persist release metadata only after complete artifact generation. Failed builds never become deployable releases.

An application owns one persistent named machine. The initial machine image contains the pinned Bun base, release and supervisor. A host-private launch descriptor and read-only release mount select subsequent code releases. On workload start, the supervisor validates the selected layer checksum, extracts into an empty guest-local staging tree, moves code into its release directory, connects reserved data paths, sets the host alias and starts Bun. The VM and `/app/var` are never replaced by normal deployments. No claim of zero downtime: switching code stops and starts the machine.

Backup stops the workload, starts the VM with no workload, archives `/app/var`, stops the maintenance VM and resumes if previously desired running. Restore verifies the checksum and application ownership, takes a safety backup, installs files while stopped, then resumes. The tar archive captures regular files, directories, modes and internal relative symlinks; it rejects device nodes, absolute/outward links and unsupported entry types. It does not capture external host shares, app environment, engine configuration, Unix ACLs or extended attributes. Back up those separately.

Reboot reconciliation restarts successfully deployed apps whose desired state is running. GUI exit has no effect. CLI wait timeouts have no effect on daemon jobs. Rootless here means the host process runs as the user; the workload is UID 0 **inside its VM** to provision paths and hosts. The implementation relies on the VM boundary, not a hardened multi-user guest.

Open release work: real-VM certification matrix, signed/notarized installers, automatic upgrades, fleet/remote control, private npm credentials, live resource/spec edits, and retention/GC policy. These are not silently emulated by this release.
