use std::io::Write;

use anyhow::{anyhow, Error};
use memmap::MmapMut;
use nix::{
    sys::{
        signal::Signal,
        wait::{waitpid, WaitStatus},
    },
    unistd::{fork, ForkResult},
};

/// Executes closure in a forked process
pub fn run_in_fork(
    f: impl FnOnce() -> Result<(), Error>,
) -> Result<(), Error> {
    let mut mmap = MmapMut::map_anon(1024).map_err(|err| {
        tracing::error!("failed to create mmap {}", err);
        anyhow!("failed to create mmap {}", err)
    })?;
    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            if let Err(err) = f() {
                err.downcast_ref::<String>()
                    .and_then(|string| {
                        (&mut mmap[..]).write_all(string.as_bytes()).ok()
                    })
                    .unwrap_or(());
                std::process::abort();
            };

            std::process::exit(0);
        }
        Ok(ForkResult::Parent { child }) => {
            let status = waitpid(child, None)?;

            match status {
                WaitStatus::Exited(_, 0) => (),
                WaitStatus::Signaled(_, Signal::SIGABRT, _) => {
                    let error = String::from_utf8_lossy(&mmap).into_owned();
                    anyhow::bail!(error);
                }
                status => {
                    anyhow::bail!("unexpected status {:?}", status);
                }
            }
        }
        Err(err) => {
            anyhow::bail!(err)
        }
    };

    Ok(())
}
