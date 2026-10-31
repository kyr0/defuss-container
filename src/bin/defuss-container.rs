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
    #[arg(long,env="DEFUSS_CONTAINER_HOME",default_value_os_t=util::home(),global=true)]
    home: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Doctor,
    List,
    Create {
        file: PathBuf,
    },
    Show {
        name: String,
    },
    Releases {
        name: String,
    },
    Backups {
        name: String,
    },
    Logs {
        name: String,
    },
    Build {
        name: String,
    },
    Deploy {
        name: String,
        release: String,
    },
    Start {
        name: String,
    },
    Stop {
        name: String,
    },
    Rollback {
        name: String,
    },
    Backup {
        name: String,
    },
    Restore {
        name: String,
        backup: String,
    },
    Delete {
        name: String,
        #[arg(long)]
        confirm: String,
    },
    Jobs,
    Job {
        id: String,
    },
    JobLog {
        id: String,
    },
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
            // Always JSON, so a text endpoint (logs) stays distinguishable from objects. VERIFIED: the GUI e2e
            // bridge (desktop/tests/e2e.test.mjs) parses every response this way.
            println!(
                "{}",
                serde_json::to_string(&client.request(&method, &path, body)?)?
            );
            return Ok(());
        }
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
