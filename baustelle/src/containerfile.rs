use std::{convert::TryFrom, fs, io::Read, path::PathBuf};

use anyhow::{Context, Error};
use dockerfile_parser::{
    Dockerfile as Containerfile, FromInstruction,
    Instruction::{self, *},
};

use futures::{
    channel::mpsc::{unbounded, SendError, UnboundedSender},
    future::{self, Future},
    stream::Stream,
    SinkExt,
    TryFutureExt,
};

use uuid::Uuid;

use registratur::v2::{
    client::Client,
    domain::{config::Config, manifest::Manifest},
};

use crate::{
    fetcher::{Fetcher, LayerDownloadStatus},
    runtime_config::RuntimeConfig,
    storage::{Storage, BLOBS_STORAGE_KEY},
    unpacker::Unpacker,
};

#[derive(Clone, Debug)]
pub enum EvaluationUpdate {
    From(LayerDownloadStatus),
}

pub struct Builder<'a> {
    fetcher: Fetcher<'a>,
    storage: &'a Storage,
    container_folder: PathBuf,
}

impl<'a> Builder<'a> {
    #[fehler::throws]
    pub fn new(
        registry_url: &'a str,
        architecture: String,
        os: Vec<String>,
        storage: &'a Storage,
    ) -> Self {
        let client = Client::build(registry_url)?;
        let fetcher = Fetcher::new(storage, client, architecture, os);
        let container_uuid = format!("{}", Uuid::new_v4());
        let container_folder =
            storage.folder().join("containers").join(&container_uuid);

        fs::create_dir_all(&container_folder)?;

        Self {
            fetcher,
            container_folder,
            storage,
        }
    }

    #[fehler::throws]
    pub fn interpret(
        &self,
        file: impl Read,
    ) -> (
        impl Stream<Item = EvaluationUpdate>,
        impl Future<Output = Result<PathBuf, Error>> + '_,
    ) {
        let (sender, receiver) = unbounded();

        let containerfile = Containerfile::from_reader(file)?;

        let result = containerfile.iter_stages().flat_map(|stage| {
            stage.instructions.into_iter().map(|instruction| {
                self.execute_instruction(instruction.clone(), sender.clone())
            })
        });

        let folder = self.container_folder.clone();

        let completion_future = future::try_join_all(result).and_then(|_| {
            future::ok(folder)
        });

        (receiver, completion_future)
    }

    #[fehler::throws]
    async fn execute_instruction(
        &self,
        instruction: Instruction,
        sender: UnboundedSender<EvaluationUpdate>,
    ) {
        match instruction {
            From(instruction) => {
                self.execute_from_instruction(instruction, sender).await?;
            }
            _ => {
                log::warn!(
                    "Unhandled containerfile instruction {:?}",
                    instruction
                )
            }
        }
    }

    #[fehler::throws]
    async fn execute_from_instruction(
        &self,
        instruction: FromInstruction,
        sender: UnboundedSender<EvaluationUpdate>,
    ) {
        let image = &instruction.image_parsed;

        let sender = sender.with(|val| {
            future::ok::<_, SendError>(EvaluationUpdate::From(val))
        });

        let default_tag = String::from("latest");
        let tag = image.tag.as_ref().unwrap_or(&default_tag);

        let digest = self.fetcher.fetch(&image.image, &tag, sender).await?;

        let manifest: Manifest =
            self.storage.get(BLOBS_STORAGE_KEY, &digest)?.context(
                "Fetched manifest was not found. Possible storage corruption",
            )?;

        let config: Config = self
            .storage
            .get(BLOBS_STORAGE_KEY, manifest.config.digest)?
            .context(
                "Fetched config was not found. Possible storage corruption",
            )?;

        let destination = self.container_folder.join("rootfs");

        let unpacker = Unpacker::new(&self.storage, &destination);

        unpacker.unpack(digest)?;

        let runtime_config =
            RuntimeConfig::try_from((config, destination.as_path()))?;

        serde_json::to_writer(
            fs::File::create(&self.container_folder.join("config.json"))?,
            &runtime_config,
        )?;
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;

    use super::*;
    use crate::storage::Storage;

    #[tokio::test]
    async fn test_interpretation() {
        #[cfg(feature = "integration_testing")]
        let (url, _mocks) = ("https://registry-1.docker.io", ());
        #[cfg(not(feature = "integration_testing"))]
        let (url, _mocks) = test_helpers::mock_server!("unix.yml");

        let tempdir = tempfile::tempdir().expect("Failed to create a tempdir");

        let storage =
            Storage::new(tempdir.path()).expect("Unable to initialize cache");

        let builder =
            Builder::new(&url, "amd64".into(), vec!["linux".into()], &storage)
                .expect("failed to initialize the builder");

        let containerfile = test_helpers::fixture!("containerfile");

        let (updates, complete_future) =
            builder.interpret(containerfile.as_bytes()).unwrap();

        let (_, result) =
            future::join(updates.collect::<Vec<_>>(), complete_future).await;

        let container_folder = result.expect("Unable to enterpret containerfile");

        assert!(container_folder.join("rootfs/etc/passwd").exists());

        let file = fs::File::open(container_folder.join("config.json"))
            .expect("Failed to open OCI runtime config file");

        let config: RuntimeConfig = serde_json::from_reader(file)
            .expect("Failed to parse OCI runtime config file");

        let command = config.process.unwrap().args.unwrap().join(" ");

        assert_eq!(command, "nginx -g daemon off;");
    }
}
