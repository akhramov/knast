use std::{
    fs::{File, OpenOptions},
    io::{copy, Error as StdError, ErrorKind},
    os::unix::{io::{FromRawFd, AsRawFd}, process::CommandExt},
    process::{self, Command, Stdio},
    thread,
};

use anyhow::Error;
use libknast::operations::{OciOperations, Process};
use nix::{
    pty::{openpty, OpenptyResult, Winsize},
    unistd::{close, dup2},
};
use serde::{Deserialize, Serialize};
use storage::StorageEngine;
use url::Url;

const CONTAINER_STDIO_STORAGE_KEY: &[u8] = b"CONTAINER_STDIO";
const CONTAINER_PTY_STATE_KEY: &[u8] = b"CONTAINER_PTY_STATE";

extern "C" {
    /// Sets winsize, used for ResizePty call
    pub fn tcsetwinsize(fd: libc::c_int, w: *mut Winsize) -> libc::c_int;
    /// Associates the terminal with the session
    pub fn tcsetsid(fd: libc::c_int, pid: libc::pid_t) -> libc::c_int;
}

#[derive(Deserialize, Serialize)]
pub struct StdioTriple {
    pub stdin: String,
    pub stdout: String,
    pub stderr: String,
    pub terminal: bool,
}

/// Containerd-specific extensions to OCI operations.
pub trait ContainerdExtension {
    /// Start needs to set up IO for process on provided files
    fn start(self, exec_id: &str) -> Result<(), Error>;
    /// Exec executes a process in the existing container
    fn exec(self, exec_id: &str, process: Process) -> Result<(), Error>;
    /// Returns stdio triple for the container.
    fn stdio_triple(&self, exec_id: &str) -> Result<StdioTriple, Error>;
    /// Persists stdio triple for the container.
    fn save_stdio_triple(
        &self,
        exec_id: &str,
        triple: StdioTriple,
    ) -> Result<(), Error>;
    /// Resizes container's PTY
    fn resize_pty(&self, exec_id: &str, winsize: Winsize)
        -> Result<(), Error>;
    /// Persists PTY master side
    fn save_pty_state(&self, exec_id: &str, pty: (i32, i32)) -> Result<(), Error>;
    /// Returns PTY state
    fn pty_state(&self, exec_id: &str) -> Result<(i32, i32), Error>;
}

impl<'a, T: StorageEngine> ContainerdExtension for OciOperations<'a, T> {
    fn resize_pty(
        &self,
        exec_id: &str,
        mut winsize: Winsize,
    ) -> Result<(), Error> {
        let (master_fd, _) = self.pty_state(exec_id)?;

        if unsafe { tcsetwinsize(master_fd, &mut winsize) < 0 } {
            anyhow::bail!(
                "tcsetwinsize() failed: {}",
                StdError::last_os_error()
            )
        }

        Ok(())
    }

    fn exec(self, exec_id: &str, process: Process) -> Result<(), Error> {
        let triple = self.stdio_triple(exec_id)?;
        self.do_exec(&exec_id, process, |command| {
            if let Some(pty) = setup_io(command, &triple)? {
                self.save_pty_state(exec_id, pty)?;
            }

            Ok(())
        })?;

        if triple.terminal {
            let (_, slave) = self.pty_state(&exec_id)?;

            close(slave)?;
        }

        Ok(())
    }

    fn start(self, exec_id: &str) -> Result<(), Error> {
        let triple = self.stdio_triple(exec_id)?;
        self.do_start(&exec_id, |command| {
            if let Some(pty) = setup_io(command, &triple)? {
                self.save_pty_state(exec_id, pty)?;
            }

            Ok(())
        })?;

        if triple.terminal {
            let (_, slave) = self.pty_state(&exec_id)?;

            close(slave)?;
        }

        Ok(())
    }

    fn stdio_triple(&self, exec_id: &str) -> Result<StdioTriple, Error> {
        self.storage()
            .get(
                CONTAINER_STDIO_STORAGE_KEY,
                [self.key().as_bytes(), b"/", exec_id.as_bytes()].concat(),
            )?
            .ok_or_else(|| anyhow::anyhow!("Container IO triple wasn't found"))
    }

    fn save_stdio_triple(
        &self,
        exec_id: &str,
        triple: StdioTriple,
    ) -> Result<(), Error> {
        self.storage().put(
            CONTAINER_STDIO_STORAGE_KEY,
            [self.key().as_bytes(), b"/", exec_id.as_bytes()].concat(),
            triple,
        )?;

        Ok(())
    }

    fn save_pty_state(&self, exec_id: &str, pty: (i32, i32)) -> Result<(), Error> {
        tracing::info!("PTY for {}/{} is {:?}", self.key(), exec_id, pty);
        self.storage().put(
            CONTAINER_PTY_STATE_KEY,
            [self.key().as_bytes(), b"/", exec_id.as_bytes()].concat(),
            pty,
        )?;

        Ok(())
    }

    fn pty_state(&self, exec_id: &str) -> Result<(i32, i32), Error> {
        self.storage()
            .get(
                CONTAINER_PTY_STATE_KEY,
                [self.key().as_bytes(), b"/", exec_id.as_bytes()].concat(),
            )?
            .ok_or_else(|| {
                anyhow::anyhow!("Container's PTY wasn't found")
            })
    }
}

fn setup_io(
    command: &mut Command,
    triple: &StdioTriple,
) -> Result<Option<(i32, i32)>, Error> {
    tracing::info!("Initializing process IO");
    let StdioTriple {
        stdin,
        stdout,
        stderr,
        terminal,
    } = triple;

    tracing::info!("Openning file descriptors");
    if *terminal {
        let mut stdin = OpenOptions::new().read(true).open(stdin)?;
        let mut stdout = OpenOptions::new().write(true).open(stdout)?;
        let OpenptyResult { master, slave } = openpty(None, None)?;
        tracing::info!("Setting up pty <-> containerd fifo pipe");
        thread::spawn(move || {
            let mut writer = unsafe { File::from_raw_fd(master) };
            let result = copy(&mut stdin, &mut writer);
            tracing::info!("Finished piping stdin with {:?}", result);
        });
        thread::spawn(move || {
            let mut reader = unsafe { File::from_raw_fd(master) };
            let result = copy(&mut reader, &mut stdout);
            tracing::info!("Finished piping stdin with {:?}", result);
        });

        unsafe {
            command.pre_exec(move || {
                let init_io = || {
                    close(master)?;
                    use libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
                    use nix::unistd::setsid;

                    setsid()?;

                    dup2(slave, STDIN_FILENO)?;
                    dup2(slave, STDOUT_FILENO)?;
                    dup2(slave, STDERR_FILENO)?;
                    if tcsetsid(slave, process::id() as _) == -1 {
                        anyhow::bail!("tcsetsid");
                    }
                    Ok(())
                };

                Ok(init_io().map_err(|_: Error| ErrorKind::Other)?)
            });
        }

        Ok(Some((master, slave)))
    } else {
        if !stdin.is_empty() {
            let stdin = OpenOptions::new().read(true).open(stdin)?;
            command.stdin(stdin);
        }

        if stdout.starts_with("binary") {
            let url = Url::parse(&stdout)?;
            let path = url.path();

            let child = Command::new(path)
                .envs(
                    url.query_pairs()
                        .map(|(k, v)| (k.to_string(), v.to_string())),
                )
                .stdin(Stdio::piped())
                .spawn()?;

            // Unwrap: stdin set previously.
            let result = child.stdin.as_ref().unwrap();
            let raw_fd = result.as_raw_fd();

            // thread::spawn(move || {
            //     child.wait().unwrap();
            // });

            let stdout = unsafe { File::from_raw_fd(raw_fd) };
            let stderr = unsafe { File::from_raw_fd(raw_fd) };

            command.stdout(stdout).stderr(stderr);

            return Ok(None);
        }

        let stdout = OpenOptions::new().write(true).open(stdout)?;
        let stderr = OpenOptions::new().write(true).open(stderr)?;

        command.stdout(stdout).stderr(stderr);
        Ok(None)
    }
}
