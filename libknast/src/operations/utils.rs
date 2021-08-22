use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
};

use anyhow::{anyhow, Error};
use nix::{
    sys::wait::{waitpid, WaitStatus},
    unistd::{fork, ForkResult},
};
use serde::{de::DeserializeOwned, ser::Serialize};

/// Executes closure in a forked process
pub fn run_in_fork<T: DeserializeOwned + Serialize>(
    f: impl FnOnce() -> Result<T, Error>,
) -> Result<T, Error> {
    let (read, mut write) = UnixStream::pair()?;

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            let result = f().map_err(|err| err.to_string());
            let result = serde_json::to_string(&result)
                .map_err(Error::from)
                .and_then(|string| {
                    write.write_all(string.as_bytes())?;
                    write.write(b"\n")?;
                    Ok(())
                });

            let status = match result {
                Ok(_) => 0,
                Err(err) => {
                    tracing::error!("run_in_fork failed: {:?}", err);

                    15
                }
            };

            std::process::exit(status);
        }
        Ok(ForkResult::Parent { child }) => {
            let status = waitpid(child, None)?;

            match status {
                WaitStatus::Exited(_, 0) => {
                    let mut string = String::new();

                    BufReader::new(read).read_line(&mut string)?;

                    let result: Result<T, String> =
                        serde_json::from_str(&string)?;

                    return result.map_err(|err| anyhow!(err));
                }
                WaitStatus::Exited(_, 15) => {
                    anyhow::bail!(
                        "Forked process failed unexpectedly. Check logs"
                    );
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
}
