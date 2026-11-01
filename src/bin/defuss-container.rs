//! The CLI: one subcommand per API route, printing the daemon's JSON (text routes such as logs
//! print raw).
//!
//! Names, specs and actions are checked with the daemon's own validation before a request is sent,
//! so a mistake gets the daemon's message without a round trip. A valid name also cannot change the
//! URL path it is placed in.
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use defuss_container::{
    client::Client,
    model::{Action, AppSpec, Job},
    util,
};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
#[derive(Parser)]
#[command(
    version,
    about = "Manage local Bun applications through defuss-containerd"
)]
struct Args {
    /// The daemon's state directory, where it publishes connection.json
    #[arg(long,env="DEFUSS_CONTAINER_HOME",default_value_os_t=util::home(),global=true)]
    home: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Engine version, provider and hypervisor checks
    Doctor,
    /// Registered apps
    List,
    /// Register an app spec from a JSON file
    Create { file: PathBuf },
    /// One app with its observed engine state
    Show { name: String },
    /// Successful builds of an app
    Releases { name: String },
    /// Backup archives of an app
    Backups { name: String },
    /// Last 200 workload log lines
    Logs { name: String },
    /// Fetch, build and store a new release
    Build { name: String },
    /// Start a release behind the health check; on failure the previous release returns
    Deploy { name: String, release: String },
    /// Set the desired state to running and start the app
    Start { name: String },
    /// Set the desired state to stopped and stop the app
    Stop { name: String },
    /// Switch to the previous code release
    Rollback { name: String },
    /// Archive the app's /app/var
    Backup { name: String },
    /// Restore a backup into /app/var, after a safety backup
    Restore { name: String, backup: String },
    /// Remove the app and its VM disk; host shares, releases and backups stay
    Delete {
        name: String,
        #[arg(long)]
        confirm: String,
    },
    /// Recent jobs
    Jobs,
    /// One job
    Job { id: String },
    /// The output log of one job
    JobLog { id: String },
    /// Wait until a job finishes; fails when it failed or was interrupted
    Wait {
        id: String,
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
    },
    /// Send one request through the client the desktop bridge uses; prints the response as JSON.
    Api {
        method: String,
        path: String,
        /// JSON request body
        #[arg(long)]
        body: Option<String>,
    },
}
fn main() -> Result<()> {
    let args = Args::parse();
    let client = Client::connect(&args.home)
        .context("cannot connect; start defuss-containerd or install the user service")?;
    let action = |name: String, a: Action| -> Result<Value> {
        anyhow::ensure!(
            defuss_container::model::valid_name(&name),
            "invalid application name"
        );
        a.validate(&name)?;
        client.request(
            "POST",
            &format!("/v1/apps/{name}/actions"),
            Some(serde_json::to_value(a)?),
        )
    };
    let result = match args.command {
        Command::Doctor => client.request("GET", "/v1/doctor", None)?,
        Command::List => client.request("GET", "/v1/apps", None)?,
        Command::Create { file } => {
            let spec: AppSpec = util::read_json(&file)?;
            spec.validate()?;
            client.request("POST", "/v1/apps", Some(serde_json::to_value(spec)?))?
        }
        Command::Show { name } => client.request("GET", &format!("/v1/apps/{name}"), None)?,
        Command::Releases { name } => {
            client.request("GET", &format!("/v1/apps/{name}/releases"), None)?
        }
        Command::Backups { name } => {
            client.request("GET", &format!("/v1/apps/{name}/backups"), None)?
        }
        Command::Logs { name } => client.request("GET", &format!("/v1/apps/{name}/logs"), None)?,
        Command::Build { name } => action(name, Action::Build)?,
        Command::Deploy { name, release } => action(name, Action::Deploy { release })?,
        Command::Start { name } => action(name, Action::Start)?,
        Command::Stop { name } => action(name, Action::Stop)?,
        Command::Rollback { name } => action(name, Action::Rollback)?,
        Command::Backup { name } => action(name, Action::Backup)?,
        Command::Restore { name, backup } => action(name, Action::Restore { backup })?,
        Command::Delete { name, confirm } => action(name, Action::Delete { confirm })?,
        Command::Jobs => client.request("GET", "/v1/jobs", None)?,
        Command::Job { id } => client.request("GET", &format!("/v1/jobs/{id}"), None)?,
        Command::JobLog { id } => client.request("GET", &format!("/v1/jobs/{id}/log"), None)?,
        Command::Api { method, path, body } => {
            let body = body.map(|b| serde_json::from_str(&b)).transpose()?;
            // Always JSON, so a text endpoint (logs) stays distinguishable from objects. VERIFIED:
            // the GUI e2e bridge (desktop/tests/e2e.test.mjs) parses every response this way.
            println!(
                "{}",
                serde_json::to_string(&client.request(&method, &path, body)?)?
            );
            return Ok(());
        }
        // Polls every 500 ms. A timeout ends only the wait: the job is durable and keeps running in
        // the daemon.
        Command::Wait { id, timeout } => {
            uuid::Uuid::parse_str(&id)?;
            let start = std::time::Instant::now();
            loop {
                let value = client.request("GET", &format!("/v1/jobs/{id}"), None)?;
                let job: Job = serde_json::from_value(value.clone())?;
                match job.state.as_str() {
                    "succeeded" => break value,
                    "failed" | "interrupted" => {
                        bail!("job {}: {}", job.state, job.error.unwrap_or_default())
                    }
                    _ => {}
                }
                if start.elapsed() > Duration::from_secs(timeout) {
                    bail!("wait timed out; job continues in daemon")
                };
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    };
    if let Value::String(s) = result {
        print!("{s}");
    } else {
        println!("{}", serde_json::to_string_pretty(&result)?);
    }
    Ok(())
}
