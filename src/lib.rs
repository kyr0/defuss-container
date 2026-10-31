pub mod api;
pub mod client;
pub mod model;
pub mod oci;
pub mod process;
pub mod provider;
pub mod service;
pub mod store;
pub mod util;

pub const ENGINE_VERSION: &str = "1.32.4";
pub const BASE_IMAGE: &str = "oven/bun:1.4.4-distroless";
pub const BUN: &str = "/usr/local/bin/bun";
pub const SUPERVISOR: &str = include_str!("../guest/supervisor.mjs");
pub const ARCHIVE_JS: &str = include_str!("../guest/archive.mjs");
pub const BUILDER: &str = include_str!("../guest/build.mjs");

pub const POLICY_JS: &str = include_str!("../guest/policy.mjs");
