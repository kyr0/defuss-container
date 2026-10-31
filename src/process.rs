use anyhow::{Context, Result, bail};
use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}
fn capture(mut stream: impl Read, log: Option<std::path::PathBuf>) -> String {
    let mut tail = Vec::new();
    let mut b = [0u8; 8192];
    let mut file = log.and_then(|p| OpenOptions::new().create(true).append(true).open(p).ok());
    while let Ok(n) = stream.read(&mut b) {
        if n == 0 {
            break;
        }
        if let Some(f) = &mut file {
            let remaining = (8 * 1024 * 1024u64)
                .saturating_sub(f.metadata().map(|m| m.len()).unwrap_or(8 * 1024 * 1024));
            if remaining > 0 {
                let _ = f.write_all(&b[..n.min(remaining as usize)]);
            }
        }
        tail.extend_from_slice(&b[..n]);
        if tail.len() > 131072 {
            tail.drain(..tail.len() - 131072);
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}
pub fn run(
    program: impl AsRef<OsStr>,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    log: Option<&Path>,
) -> Result<Output> {
    let mut c = Command::new(program);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(p) = cwd {
        c.current_dir(p);
    }
    for key in [
        "SMOL_CLOUD_TOKEN",
        "SMOLVM_MACHINE_NAME",
        "BUN_OPTIONS",
        "NODE_OPTIONS",
        "LD_PRELOAD",
    ] {
        c.env_remove(key);
    }
    c.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let mut child = c.spawn().context("launch child process")?;
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let l = log.map(Path::to_path_buf);
    let l2 = l.clone();
    let a = thread::spawn(move || capture(out, l));
    let b = thread::spawn(move || capture(err, l2));
    let start = Instant::now();
    let mut timed = false;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if start.elapsed() > timeout {
            timed = true;
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill.exe")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .output();
            }
            let _ = child.kill();
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let stdout = a.join().unwrap_or_default();
    let stderr = b.join().unwrap_or_default();
    if timed {
        bail!("command timed out after {}s", timeout.as_secs())
    }
    Ok(Output {
        stdout,
        stderr,
        code: status.code().unwrap_or(-1),
    })
}
pub fn checked(
    program: impl AsRef<OsStr>,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
    log: Option<&Path>,
) -> Result<String> {
    let r = run(program, args, cwd, timeout, log)?;
    if r.code != 0 {
        bail!(
            "child exited {}: {}",
            r.code,
            r.stderr
                .chars()
                .rev()
                .take(4096)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        )
    }
    Ok(r.stdout)
}
