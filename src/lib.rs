//! defuss-container: build, deploy and run Bun applications, each in its own persistent Linux
//! microVM.
//!
//! The daemon (`defuss-containerd`) owns all state and is the only process that runs Git or the VM
//! engine. The CLI (`defuss-container`) and the desktop bridge are thin HTTP clients built on
//! [`client`]. ARCH.md explains why the work is split this way.
//!
//! Layout: [`api`] is the loopback HTTP surface, [`service`] runs jobs (build, deploy, backup,
//! ...), [`provider`] drives the smolvm CLI, [`oci`] fetches the base image and assembles release
//! images, [`store`] keeps apps, releases and jobs in SQLite, [`model`] holds the validated data
//! types, and [`process`] and [`util`] wrap child processes and private files.
pub mod api;
pub mod client;
pub mod model;
pub mod oci;
pub mod process;
pub mod provider;
pub mod service;
pub mod store;
pub mod util;

/// The only engine release the adapter accepts. VERIFIED: every flag it passes was checked against
/// this version's `--help` (docs/VERIFICATION.md, "Engine CLI contract"); scripts/install.py pins
/// its archive checksums.
pub const ENGINE_VERSION: &str = "1.32.4";
/// Base image of every builder and app VM. The image has no `/bin/sh`, which is why guest code runs
/// package scripts with `bun exec` (ARCH.md, "Runtime").
pub const BASE_IMAGE: &str = "oven/bun:1.4.4-distroless";
/// Bun's absolute path inside the base image: the image entrypoint and every `machine exec` name
/// it.
pub const BUN: &str = "/usr/local/bin/bun";

// Guest programs are compiled into the binaries, so an installed daemon needs no guest files on
// disk and always writes the version it was built with into builder shares and image layers.
/// Runs in every app VM: verifies and unpacks the release, links the data directories, starts
/// `scripts.start`.
pub const SUPERVISOR: &str = include_str!("../guest/supervisor.mjs");
/// Tar reader and writer for release layers and `/app/var` backups, shared by the builder and the
/// supervisor.
pub const ARCHIVE_JS: &str = include_str!("../guest/archive.mjs");
/// Runs in the disposable builder VM: frozen install, optional build script, production reinstall,
/// pack.
pub const BUILDER: &str = include_str!("../guest/build.mjs");
/// Rules shared by builder and supervisor, such as how a package script is run (`bun exec`).
pub const POLICY_JS: &str = include_str!("../guest/policy.mjs");
