use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Error, Result};
use registratur::v2::domain::manifest::Manifest;

use super::archive::Archive;
use super::storage::{Storage, StorageEngine, BLOBS_STORAGE_KEY};

pub struct Unpacker<'a, T: StorageEngine> {
    storage: &'a Storage<T>,
    destination: &'a Path,
}

impl<'a, T: StorageEngine> Unpacker<'a, T> {
    pub fn new(storage: &'a Storage<T>, destination: &'a Path) -> Self {
        Self {
            storage,
            destination,
        }
    }

    #[fehler::throws]
    pub fn unpack(&self, digest: String) {
        let maybe_manifest: Option<Manifest> =
            self.storage.get(BLOBS_STORAGE_KEY, digest)?;

        if let Some(manifest) = maybe_manifest {
            manifest
                .layers
                .into_iter()
                .map(|layer| self.unpack_layer(layer.digest))
                .collect::<Result<Vec<_>>>()?;
        } else {
            fehler::throw!(anyhow!("Image is not cached"));
        }
    }

    #[fehler::throws]
    fn unpack_layer(&self, digest: String) {
        let maybe_digest: Option<Vec<u8>> =
            self.storage.get(BLOBS_STORAGE_KEY, digest)?;

        if let Some(layer) = maybe_digest {
            let archive = Archive::new(&layer);

            self.handle_whiteouts(&archive)?;
            archive.extract(&self.destination, |entry| {
                match Path::new(&entry).file_name() {
                    None => false,
                    Some(name) => name.to_string_lossy().starts_with(".wh."),
                }
            })?;
        } else {
            fehler::throw!(anyhow!(
                "Layer is not cached. DB might be corrupted"
            ));
        }
    }

    #[fehler::throws]
    fn handle_whiteouts(&self, archive: &Archive) {
        archive
            .entries()?
            .map(|maybe_entry| {
                maybe_entry.map::<Result<()>, _>(|entry: PathBuf| {
                    let filename = entry.file_name().context(
                        "Failed to extract filename from the archive header",
                    )?;
                    let parent = entry.parent().context(
                        "Failed to extract dirname from the archive header",
                    )?;

                    let parent = self.destination.join(parent);
                    let entry = self.destination.join(&entry);

                    match &*filename.to_string_lossy() {
                        ".wh..wh..opq" => fs::remove_dir_all(parent)?,
                        item if item.starts_with(".wh.") => {
                            fs::remove_file(&entry)?
                        }
                        _ => (),
                    };

                    Ok(())
                })
            })
            .collect::<Result<Vec<_>>>()?
    }
}

#[cfg(test)]
mod test {
    use std::{fs, path::PathBuf};

    use registratur::v2::client::Client;

    use super::Unpacker;
    use crate::{fetcher::Fetcher, storage::TestStorage as Storage};

    #[tokio::test]
    #[cfg(feature = "integration_testing")]
    async fn test_unpacking() {
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

        let destination = tempdir.into_path().join(&digest);
        let unpacker = Unpacker::new(&storage, &destination);

        unpacker
            .unpack(digest)
            .expect("Failed to unpack the archive");
    }

    #[tokio::test]
    #[cfg(not(feature = "integration_testing"))]
    async fn test_unpacking() {
        #[fehler::throws(anyhow::Error)]
        fn visit_dirs(
            dir: &PathBuf,
            mut result: Vec<PathBuf>,
        ) -> Vec<PathBuf> {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    result = visit_dirs(&path, result)?;
                } else {
                    result.push(path);
                }
            }

            result
        }

        let (url, _mocks) = test_helpers::mock_server!("whiteouts.yml");

        let tempdir = tempfile::tempdir().expect("Failed to create a tempdir");

        let storage =
            Storage::new(&tempdir.path()).expect("Unable to initialize cache");

        let digest = {
            let client =
                Client::build(&url).expect("failed to build the client");

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

        let destination = tempdir.into_path().join(&digest);
        let unpacker = Unpacker::new(&storage, &destination);

        unpacker
            .unpack(digest)
            .expect("Failed to unpack the archive");

        let mut result = visit_dirs(&destination, vec![])
            .expect("Failed to read the directory")
            .into_iter()
            .map(|x| x.strip_prefix(&destination).unwrap().to_path_buf())
            .collect::<Vec<_>>();

        let expected = test_helpers::code_fixture!("unpacked_layers");

        result.sort();

        assert_eq!(result, expected);
    }
}
