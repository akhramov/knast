use std::sync::Arc;

use anyhow::{Context, Error};
use futures::{
    executor::block_on,
    future::{self, TryFutureExt},
    sink::{Sink, SinkExt},
    stream::{FuturesUnordered, TryStreamExt},
};
use registratur::v2::{
    client::Client,
    domain::{
        config::Config,
        layer::Layer,
        manifest::Manifest,
        manifest_index::{ManifestIndex, Platform},
    },
};

use super::storage::{
    Storage, StorageEngine, BLOBS_STORAGE_KEY, IMAGES_INDEX_STORAGE_KEY,
};

/// Represents layer download update.
#[derive(Clone, Debug)]
pub enum LayerDownloadStatus {
    Cached(Arc<String>),
    InProgress(Arc<String>, usize, usize),
}

pub struct Fetcher<'a, T: StorageEngine> {
    storage: &'a Storage<T>,
    client: Client<'a>,
    architecture: String,
    os: Vec<String>, /* We support Linux & FreeBSD containers running
                      * alongside */
}

impl<'a, T: StorageEngine> Fetcher<'a, T> {
    pub fn new(
        storage: &'a Storage<T>,
        client: Client<'a>,
        architecture: String,
        os: Vec<String>,
    ) -> Self {
        Self {
            storage,
            client,
            architecture,
            os,
        }
    }

    /// Fetches the image, including it's configuration and
    /// layer from the registry.
    ///
    /// # Example
    ///
    /// Fetch nginx image, retrieving updates via a mpsc
    /// channel
    ///
    /// ```rust,no_run
    /// use futures::{future, stream::StreamExt};
    /// use registratur::v2::client::Client;
    /// use baustelle::{fetcher::{Fetcher, LayerDownloadStatus::*}, storage::Storage};
    ///
    /// let storage =
    ///     Storage::new("/opt/dir").expect("Unable to initialize cache");
    /// let client = Client::build("https://registry-1.docker.io")
    ///     .expect("failed to build the client");
    ///
    /// let architecture = "amd64";
    /// let os = vec!["linux".into(), "freebsd".into()];
    /// let fetcher = Fetcher::new(&storage, client, architecture.into(), os);
    /// let (tx, rx) = futures::channel::mpsc::channel(1);
    ///
    /// async {
    ///     let digest_fut = fetcher.fetch("nginx", "1.17.10", tx);
    ///     let updates_fut = rx.collect::<Vec<_>>();
    ///
    ///     let (digest, updates) = future::join(digest_fut, updates_fut).await;
    ///
    ///     updates.iter().for_each(|x| {
    ///         if let InProgress(name, count, total) = x {
    ///             println!("{} downloaded {} of {}", name, count, total);
    ///         }
    ///     });
    ///
    ///     println!("Fetched an image. Its digest is {:?}", digest);
    /// };
    /// ```
    #[fehler::throws]
    pub async fn fetch(
        &self,
        image: &str,
        tag: &str,
        updates_sub: impl Sink<LayerDownloadStatus> + Clone + Unpin + Send,
    ) -> String {
        let image_name = normalize_image_name(image);
        let cache_key = &format!("{}:{}", image_name, tag)[..];

        if let Some(digest) =
            self.storage.get(IMAGES_INDEX_STORAGE_KEY, cache_key)?
        {
            return digest;
        };

        let digest = self.resolve_manifest_digest(&image_name, tag).await?;

        self.fetch_manifest(&image_name, &digest)
            .and_then(|manifest| {
                let layers: FuturesUnordered<_> = manifest
                    .layers
                    .into_iter()
                    .map(|layer| {
                        self.fetch_layer(
                            &image_name,
                            layer.digest,
                            layer.size,
                            updates_sub.clone(),
                        )
                    })
                    .collect();

                let config =
                    self.fetch_config(&image_name, manifest.config.digest);

                future::try_join(config, layers.try_collect::<Vec<_>>())
            })
            .await?;

        self.storage
            .put(IMAGES_INDEX_STORAGE_KEY, cache_key, &digest)?;
        self.storage.flush().await?;

        digest
    }

    #[fehler::throws]
    async fn resolve_manifest_digest(
        &self,
        image_name: &str,
        tag: &str,
    ) -> String {
        let Self {
            client,
            architecture,
            os,
            ..
        } = self;

        let manifests = ManifestIndex::pull(client, image_name, tag)
            .await
            .context(format!("Failed to fetch manifest index {}", image_name))?
            .manifests;

        manifests
            .iter()
            .find(|ref manifest| match &manifest.platform {
                Some(Platform {
                    architecture: img_arch,
                    os: img_os,
                    ..
                }) => architecture == img_arch && os.contains(&img_os),
                None => false,
            })
            .map(|manifest| manifest.descriptor.digest.clone())
            .context(format!(
                "Could not find the appropriate manifest for: {} ({:?})",
                architecture, os,
            ))?
    }

    #[fehler::throws]
    async fn fetch_manifest(
        &self,
        image_name: &str,
        digest: &str,
    ) -> Manifest {
        Manifest::pull(&self.client, image_name, digest)
            .await
            .and_then(|item| self.storage.put(BLOBS_STORAGE_KEY, digest, item))
            .context(format!(
                "Failed to fetch manifest {} {}",
                image_name, digest
            ))?
    }

    #[fehler::throws]
    async fn fetch_layer(
        &self,
        image_name: &str,
        digest: String,
        size: usize,
        mut updates_sub: impl Sink<LayerDownloadStatus> + Clone + Unpin + Send,
    ) {
        let digest_arc = Arc::new(digest.clone());

        if self.storage.exists(BLOBS_STORAGE_KEY, &digest)? {
            // This may fail for various reason, but we don't care,
            // since it is a UI code and UI does not handle
            // the progress retrieval failures.
            let _ = block_on(
                updates_sub
                    .send(LayerDownloadStatus::Cached(digest_arc.clone())),
            );

            return;
        }

        let updates_handler = move |x| {
            // This may fail for various reason, but we don't care,
            // since it is a UI code and UI does not handle
            // the progress retrieval failures.
            let _ = block_on(updates_sub.send(
                LayerDownloadStatus::InProgress(digest_arc.clone(), x, size),
            ));
        };

        Layer::pull(&self.client, &image_name, &digest, updates_handler)
            .await
            .and_then(|item| {
                self.storage.put(BLOBS_STORAGE_KEY, &digest, item)
            })
            .context(format!("Failed to fetch layer {}", digest))?;
    }

    #[fehler::throws]
    async fn fetch_config(&self, image_name: &str, digest: String) {
        Config::pull(&self.client, &image_name, &digest)
            .await
            .and_then(|item| {
                self.storage.put(BLOBS_STORAGE_KEY, &digest, item)
            })
            .context(format!("Failed to fetch image config {}", digest))?;
    }
}

fn normalize_image_name(image: &str) -> String {
    let prefix = if image.contains('/') { "" } else { "library/" };

    format!("{}{}", prefix, image)
}

#[cfg(test)]
mod test {
    use futures::stream::StreamExt;

    use super::*;
    use crate::storage::TestStorage as Storage;

    macro_rules! setup_client {
        ($var:ident, $fetcher:ident, $dir:ident) => {
            #[cfg(feature = "integration_testing")]
            let (url, _mocks) = ("https://registry-1.docker.io", ());
            #[cfg(not(feature = "integration_testing"))]
            let (url, _mocks) = test_helpers::mock_server!("basic.yml");

            let $dir =
                tempfile::tempdir().expect("failed to create a tmp directory");

            let storage =
                Storage::new($dir.path()).expect("Unable to initialize cache");

            let architecture = "amd64";

            let os = vec!["linux".into(), "freebsd".into()];

            let $var =
                Client::build(&url).expect("failed to build the client");

            let $fetcher =
                Fetcher::new(&storage, $var, architecture.into(), os);
        };
    }

    use registratur::v2::{client::Client, domain::manifest::Manifest};

    fn get_manifest_from_storage(
        storage: &Storage,
        key: &str,
    ) -> Manifest {
        let image_digest: String =
            storage.get(IMAGES_INDEX_STORAGE_KEY, key).unwrap().unwrap();

        storage
            .get(BLOBS_STORAGE_KEY, image_digest)
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn integration_test_fetch_image() {
        setup_client!(client, fetcher, dir);

        let (tx, _) = futures::channel::mpsc::channel(1);

        fetcher
            .fetch("nginx", "1.17.10", tx)
            .await
            .expect("Failed to fetch image");

        let storage =
            Storage::new(dir.path()).expect("Unable to initialize cache");

        let manifest =
            get_manifest_from_storage(&storage, "library/nginx:1.17.10");

        let config_digest = manifest.config.digest;

        let config: Config = storage
            .get(BLOBS_STORAGE_KEY, config_digest)
            .unwrap()
            .unwrap();

        assert_eq!("amd64", config.architecture);
    }

    #[tokio::test]
    async fn integration_test_progress() {
        setup_client!(client, fetcher, dir);

        let (tx, rx) = futures::channel::mpsc::channel(100);

        let progress_future = rx.collect::<Vec<_>>();
        let fetcher_future = fetcher.fetch("nginx", "1.17.10", tx);

        let (image, progress_items) =
            future::join(fetcher_future, progress_future).await;

        image.expect("Failed to fetch image");

        let storage =
            Storage::new(dir.path()).expect("Unable to initialize cache");

        let mut downloaded_layers = progress_items.iter().fold(
            Vec::<String>::new(),
            |mut acc, item| match item {
                LayerDownloadStatus::InProgress(layer, x, y) if x == y => {
                    acc.push(layer.to_string());
                    acc
                }
                _ => acc,
            },
        );

        let mut stored_layers =
            get_manifest_from_storage(&storage, "library/nginx:1.17.10")
                .layers
                .into_iter()
                .map(|layer| layer.digest)
                .collect::<Vec<_>>();

        downloaded_layers.sort();
        stored_layers.sort();

        assert_eq!(stored_layers, downloaded_layers);
    }
}
