mod fetcher;
mod runtime_config;
mod storage;
mod unpacker;

mod containerfile;

mod archive;

use std::{convert::AsRef, io::Read, path::{Path, PathBuf}};

use anyhow::Error;
use futures::{future, StreamExt};

use containerfile::{Builder as ContainerfileBuilder};
pub use containerfile::EvaluationUpdate;
pub use fetcher::LayerDownloadStatus;
use storage::Storage;

pub struct Builder {
    architecture: String,
    os: Vec<String>,
    storage: Storage,
}

impl Builder {
    #[fehler::throws]
    pub fn new(
        architecture: String,
        os: Vec<String>,
        path: impl AsRef<Path>,
    ) -> Self {
        let storage = Storage::new(&path)?;

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
    fn construct_builder() -> (Builder, TempDir) {
        let tmpdir = tempfile::tempdir().unwrap();

        (
            Builder::new("amd64".into(), vec!["linux".into()], tmpdir.path())?,
            tmpdir,
        )
    }
}
