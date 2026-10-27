use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    os::unix::{
        fs::OpenOptionsExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

struct OwnedGroup {
    pid: libc::pid_t,
    killed: AtomicBool,
}
impl OwnedGroup {
    fn terminate_once(&self) {
        if !self.killed.swap(true, Ordering::AcqRel) {
            // The leader PID is reserved until cleanup completes (waitid WNOWAIT).
            unsafe {
                libc::kill(-self.pid, libc::SIGKILL);
            }
        }
    }
}

fn observe_exit_without_reaping(pid: libc::pid_t) -> io::Result<()> {
    loop {
        // waitid fills this plain C structure; we do not inspect its union fields.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

pub fn run() -> io::Result<u8> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--ready-file")) {
        return Err(io::Error::other(
            "usage: hive-command-guard --ready-file PATH -- PROGRAM [ARGS]",
        ));
    }
    let ready = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("missing ready path"))?,
    );
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--")) {
        return Err(io::Error::other("missing command separator"));
    }
    let program = args
        .next()
        .ok_or_else(|| io::Error::other("missing program"))?;
    // Keep the guardian out of the worker's process group. A caller may already
    // have created a new session; otherwise establish one before launching work.
    if unsafe { libc::getsid(0) } != unsafe { libc::getpid() } && unsafe { libc::setsid() } == -1 {
        return Err(io::Error::last_os_error());
    }
    let mut child = Command::new(program)
        .args(args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    let group = Arc::new(OwnedGroup {
        pid: child.id() as libc::pid_t,
        killed: AtomicBool::new(false),
    });
    let watched = group.clone();
    if let Err(error) = std::thread::Builder::new()
        .name("worker-liveness".into())
        .spawn(move || {
            // Only the worker retains the write end. Tool stdin is /dev/null, so
            // descendants cannot accidentally keep this liveness pipe open.
            let mut input = std::io::stdin().lock();
            let mut byte = [0u8; 1];
            loop {
                match input.read(&mut byte) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            watched.terminate_once();
        })
    {
        group.terminate_once();
        let _ = child.wait();
        return Err(error);
    }
    let announce = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(ready)?;
        writeln!(
            file,
            "{{\"guardian\":{},\"command_group\":{}}}",
            std::process::id(),
            child.id()
        )
    })();
    if let Err(error) = announce {
        group.terminate_once();
        let _ = child.wait();
        return Err(error);
    }
    if let Err(error) = observe_exit_without_reaping(group.pid) {
        group.terminate_once();
        let _ = child.wait();
        return Err(error);
    }
    // Kill remaining descendants before reaping the leader, so a reused PID
    // cannot redirect cleanup onto an unrelated process group.
    group.terminate_once();
    let status = child.wait()?;
    Ok(status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
        .clamp(0, 255) as u8)
}
