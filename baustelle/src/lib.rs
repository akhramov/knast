mod fetcher;
pub mod runtime_config;
mod storage;
mod unpacker;

mod containerfile;

mod archive;

use std::{io::Read, path::PathBuf};

use anyhow::Error;
use futures::{future, StreamExt};

use crate::storage::{Storage, StorageEngine};
use containerfile::Builder as ContainerfileBuilder;
pub use containerfile::EvaluationUpdate;
pub use fetcher::LayerDownloadStatus;

pub struct Builder<T: StorageEngine> {
    architecture: String,
    os: Vec<String>,
    storage: Storage<T>,
}

impl<T: StorageEngine> Builder<T> {
    #[fehler::throws]
    pub fn new(
        architecture: String,
        os: Vec<String>,
        storage: Storage<T>,
    ) -> Self {
        Self {
            architecture,
            os,
            storage,
        }
    }

    #[fehler::throws]
    pub async fn build(
        &self,
        registry: &str,
        containerfile: impl Read,
        callback: impl Fn(EvaluationUpdate),
    ) -> PathBuf {
        let Self {
            architecture,
            os,
            storage,
        } = self;

        let builder = ContainerfileBuilder::new(
            registry,
            architecture.into(),
            os.to_vec(),
            &storage,
        )?;

        let (updates, future) = builder.interpret(containerfile)?;

        let updates = updates.for_each(|item| {
            callback(item);

            future::ready(())
        });

        let (result, _) = future::join(future, updates).await;

        result?
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::storage::{StorageEngine, TestStorage as Storage};

    #[test]
    fn test_image_build_initializer() {
        let builder = construct_builder();

        assert!(builder.is_ok(), "Failed to create builder")
    }

    #[tokio::test]
    async fn test_image_building_api() {
        #[cfg(feature = "integration_testing")]
        let (url, _mocks) = ("https://registry-1.docker.io", ());
        #[cfg(not(feature = "integration_testing"))]
        let (url, _mocks) = test_helpers::mock_server!("unix.yml");

        let (builder, _path) =
            construct_builder().expect("failed to create builder");

        let containerfile = test_helpers::fixture!("containerfile");
        let container_folder = builder
            .build(&url, containerfile.as_bytes(), |_| {})
            .await
            .unwrap();

        assert!(container_folder.join("rootfs/etc/passwd").exists());
    }

    #[fehler::throws]
    fn construct_builder() -> (Builder<impl StorageEngine>, TempDir) {
        let tmpdir = tempfile::tempdir().unwrap();
        let storage = Storage::new(tmpdir.path()).unwrap();

        (
            Builder::new("amd64".into(), vec!["linux".into()], storage)?,
            tmpdir,
        )
    }
}
