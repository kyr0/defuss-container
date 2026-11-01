//! The daemon: the only process that runs Git or the VM engine and the owner of all state in its
//! home directory.
//!
//! Start-up order matters: lock the home directory, open the store (which marks unfinished jobs
//! interrupted), bind loopback, publish a fresh token, then queue a start or stop per app to
//! restore its desired state. Shutdown on SIGTERM or Ctrl-C waits for running jobs before releasing
//! the lock.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use defuss_container::{
    api::{self, ApiState},
    model::{Action, Connection},
    service::Service,
    util,
};
use fs2::FileExt;
use rand::RngCore;
use std::{fs::OpenOptions, path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(version, about = "Local defuss-container HTTP daemon")]
struct Args {
    /// State directory (apps, releases, jobs, backups); private to the user and never a symlink
    #[arg(long,env="DEFUSS_CONTAINER_HOME",default_value_os_t=util::home())]
    home: PathBuf,
    /// smolvm executable [default: <home>/engine/smolvm, where scripts/install.py puts it]
    #[arg(long, env = "DEFUSS_CONTAINER_ENGINE")]
    engine: Option<PathBuf>,
    /// Loopback API port; 0 lets the OS pick one (tests). Clients find it in connection.json either
    /// way.
    #[arg(long, env = "DEFUSS_CONTAINER_PORT", default_value_t = 63705)]
    port: u16,
}
#[tokio::main]
async fn main() -> Result<()> {
    // Plain text when stdout is a log file (launchd, systemd, e2e): escape codes are noise there.
    // VERIFIED: without RUST_LOG the daemon logged nothing, and the service units set none; default
    // to info.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();
    let args = Args::parse();
    util::private_dir(&args.home)?;
    let home = dunce::canonicalize(&args.home)?;
    util::ensure_no_symlinks(&home)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(home.join("daemon.lock"))?;
    // An OS file lock, not a pid file: the kernel drops it when the process dies, so a crash never
    // leaves a stale owner, and a second daemon on the same home fails here before touching the
    // store.
    lock.try_lock_exclusive()
        .context("another daemon owns this data directory")?;
    let engine = args.engine.unwrap_or_else(|| {
        home.join("engine").join(if cfg!(windows) {
            "smolvm.exe"
        } else {
            "smolvm"
        })
    });
    let engine = if engine.is_absolute() {
        engine
    } else {
        std::env::current_dir()?.join(engine)
    };
    let service = Arc::new(Service::open(home.clone(), engine)?);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, args.port))
        .await
        .with_context(|| {
            format!(
                "cannot bind 127.0.0.1:{}; choose another port with --port or DEFUSS_CONTAINER_PORT (0 = any free port)",
                args.port
            )
        })?;
    let address = listener.local_addr()?;
    ensure!(address.ip().is_loopback(), "daemon must bind loopback");
    // A fresh token per start: a crashed daemon's stale connection.json must not hold a credential
    // that is still valid for whichever process binds its old loopback port next.
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    // connection.json is the only copy. Earlier versions also wrote `token`, which no client read.
    let _ = std::fs::remove_file(home.join("token"));
    util::write_json(
        &home.join("connection.json"),
        &Connection {
            address: format!("http://{address}"),
            token: token.clone(),
        },
    )?;
    tracing::info!(%address,"daemon listening");
    // Reconcile persisted desired state after reboot/interrupted deployment. Startup is
    // asynchronous.
    for app in service.store.apps()? {
        let action = if app.desired_running {
            Action::Start
        } else {
            Action::Stop
        };
        let _ = service.enqueue(&app.spec.name, action);
    }
    let router = api::router(ApiState {
        service: service.clone(),
        token,
        authority: address.to_string(),
    });
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown())
        .await?;
    // Keep the process lock while blocking workers finish. If the OS kills us,
    // SQLite recovery marks their jobs interrupted on the next startup.
    // 2 is the number of job workers; holding every permit means no job is still running.
    let _drained = service.gate.clone().acquire_many_owned(2).await?;
    service.gate.close();
    let _ = std::fs::remove_file(home.join("connection.json"));
    drop(lock);
    Ok(())
}
async fn shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal handler");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
