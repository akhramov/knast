mod user;

use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::path::{Path, PathBuf};

use anyhow::Error;
use registratur::v2::domain::config;
use serde::{Deserialize, Serialize};

/// Represents [OCI Container Configuration file](https://github.com/opencontainers/runtime-spec/blob/v1.0.0/config.md)
#[derive(Deserialize, Serialize, Debug)]
pub struct RuntimeConfig {
    #[serde(rename = "ociVersion")]
    pub oci_version: String,
    pub root: Option<Root>,
    pub mounts: Option<Vec<Mount>>,
    pub process: Option<Process>,
    pub hooks: Option<Hooks>,
    pub annotations: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Root {
    pub path: PathBuf,
    pub readonly: Option<bool>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Mount {
    pub destination: String,
    pub source: Option<String>,
    pub options: Option<Vec<String>>,
    pub r#type: Option<String>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Process {
    pub terminal: Option<bool>,
    #[serde(rename = "consoleSize")]
    pub console_size: Option<ConsoleSize>,
    pub cwd: String,
    pub env: Option<Vec<String>>,
    pub args: Option<Vec<String>>,
    pub rlimits: Option<Vec<Rlimit>>,
    pub user: User,
    pub hostname: Option<String>,
    /* commandLine omitted */
}

#[derive(Deserialize, Serialize, Debug)]
pub struct ConsoleSize {
    pub height: u32,
    pub width: u32,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Rlimit {
    pub r#type: String,
    pub soft: u32,
    pub hard: u32,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct User {
    pub uid: u32,
    pub gid: u32,
    pub umask: Option<u32>,
    #[serde(rename = "additionalGids")]
    pub additional_gids: Option<Vec<u32>>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Hooks {
    pub prestart: Option<Vec<Hook>>,
    #[serde(rename = "createRuntime")]
    pub create_runtime: Option<Vec<Hook>>,
    #[serde(rename = "createContainer")]
    pub create_container: Option<Vec<Hook>>,
    #[serde(rename = "startContainer")]
    pub start_container: Option<Vec<Hook>>,
    pub poststart: Option<Vec<Hook>>,
    pub poststop: Option<Vec<Hook>>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Hook {
    pub path: String,
    pub args: Option<Vec<String>>,
    pub env: Option<Vec<String>>,
    pub timeout: Option<u32>,
}

impl TryFrom<(config::Config, &Path)> for RuntimeConfig {
    type Error = Error;

    #[fehler::throws]
    fn try_from((config, rootfs): (config::Config, &Path)) -> Self {
        let annotations = generate_annotations();
        let process = config
            .config
            .map(|config| Process::try_from((config, rootfs)))
            .transpose()?;

        Self {
            oci_version: "1.0".into(),
            root: Some(rootfs.try_into()?),
            mounts: None,
            process,
            hooks: None,
            annotations: Some(annotations),
        }
    }
}

fn generate_annotations() -> HashMap<String, String> {
    let mut annotations = HashMap::new();

    // TODO: something meaningful, or at least adhere to OCI
    // spec :)
    annotations.insert("io.container.manager".into(), "werft".into());
    annotations
        .insert("org.opencontainers.image.stopSignal".into(), "15".into());

    annotations
}

impl TryFrom<&Path> for Root {
    type Error = Error;

    #[fehler::throws]
    fn try_from(rootfs: &Path) -> Self {
        Self {
            path: rootfs.into(),
            readonly: Some(false),
        }
    }
}

impl TryFrom<(config::Container, &Path)> for Process {
    type Error = Error;

    #[fehler::throws]
    fn try_from((config, rootfs): (config::Container, &Path)) -> Self {
        let args = [
            config.entrypoint.unwrap_or(vec![]),
            config.cmd.unwrap_or(vec![]),
        ]
        .concat();

        Self {
            terminal: None,
            console_size: None,
            cwd: config.working_dir,
            env: config.env,
            args: Some(args),
            rlimits: None,
            user: (config.user, rootfs).try_into()?,
            hostname: None,
        }
    }
}

impl TryFrom<(Option<String>, &Path)> for User {
    type Error = Error;

    #[fehler::throws]
    fn try_from((user, rootfs): (Option<String>, &Path)) -> Self {
        let (uid, gid) = match user {
            Some(user) if user.len() > 0 => user::parse(user, rootfs)?,
            _ => (0, 0),
        };

        Self {
            uid,
            gid,
            umask: None,
            additional_gids: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use registratur::v2::domain::{config::Config, manifest::Manifest};

    use super::*;

    #[test]
    fn test_deserialization() {
        let fixture = test_helpers::fixture!("runtime_config.json");

        let config: RuntimeConfig = serde_json::from_str(fixture)
            .expect("failed to deserialize runtime config");

        assert_eq!(
            config.process.unwrap().rlimits.unwrap()[0].r#type,
            "RLIMIT_NOFILE"
        );

        assert_eq!(
            config.mounts.unwrap()[0].options.as_ref().unwrap()[0],
            "nosuid"
        );
    }

    #[test]
    #[cfg(not(feature = "integration_testing"))]
    fn test_conversion() {
        let fixture = test_helpers::fixture!("config.json");

        let config: Config = serde_json::from_str(fixture).unwrap();
        let path = test_helpers::fixture_path!("unix/happy_path");

        let runtime_config = RuntimeConfig::try_from((config, path)).unwrap();

        let process = runtime_config.process.unwrap();
        let User { uid, gid, .. } = process.user;
        let env_var = &process.env.unwrap()[1];

        assert_eq!(env_var, "NGINX_VERSION=1.17.10");
        assert_eq!((uid, gid), (977, 13));
    }

    // TODO: I really don't like the body of this test... Like,
    // really.
    #[tokio::test]
    #[cfg(feature = "integration_testing")]
    async fn test_conversion() {
        use crate::{
            fetcher::Fetcher,
            storage::{Storage, BLOBS_STORAGE_KEY},
            unpacker::Unpacker,
        };

        use registratur::v2::client::Client;

        let tempdir = tempfile::tempdir().expect("Failed to create a tempdir");

        let storage =
            Storage::new(tempdir.path()).expect("Unable to initialize cache");

        let digest = {
            let client = Client::build("https://registry-1.docker.io")
                .expect("failed to build the client");

            let architecture = "amd64";
            let os = vec!["linux".into(), "freebsd".into()];
            let fetcher =
                Fetcher::new(&storage, client, architecture.into(), os);
            let (tx, _) = futures::channel::mpsc::channel(1);

            fetcher
                .fetch("nginx", "1.17.10", tx)
                .await
                .expect("Failed to fetch the image")
        };

        let manifest: Manifest =
            storage.get(BLOBS_STORAGE_KEY, &digest).unwrap().unwrap();

        let config: Config = storage
            .get(BLOBS_STORAGE_KEY, manifest.config.digest)
            .unwrap()
            .unwrap();

        let destination = tempdir.into_path().join(&digest);
        let unpacker = Unpacker::new(&storage, &destination);

        unpacker
            .unpack(digest)
            .expect("Failed to unpack the archive");

        let runtime_config =
            RuntimeConfig::try_from((config, Path::new(&destination)))
                .unwrap();

        let process = runtime_config.process.unwrap();
        let User { uid, gid, .. } = process.user;
        let env_var = &process.env.unwrap()[1];

        assert_eq!(env_var, "NGINX_VERSION=1.17.10");
        assert_eq!((uid, gid), (0, 0));
    }
}
