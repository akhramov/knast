use std::path::Path;

use anyhow::Error;
use async_trait::async_trait;

use super::StorageEngine;

const STORAGE_FILE: &str = "storage.db";

#[async_trait]
impl StorageEngine for sled::Db {
    #[fehler::throws]
    fn initialize(cache_dir: impl AsRef<Path>) -> Box<Self> {
        Box::new(sled::open(cache_dir.as_ref().join(STORAGE_FILE))?)
    }

    #[fehler::throws]
    fn get(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> Option<Vec<u8>> {
        let tree = self.open_tree(collection)?;

        tree.get(key)?.map(|x| (*x).to_vec())
    }

    #[fehler::throws]
    fn put(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) {
        let tree = self.open_tree(collection)?;

        tree.insert(key.as_ref(), value.as_ref())?;
    }

    #[fehler::throws]
    fn compare_and_swap(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        old_value: impl AsRef<[u8]>,
        new_value: impl AsRef<[u8]>,
    ) {
        let tree = self.open_tree(collection)?;

        tree.compare_and_swap(
            key.as_ref(),
            Some(old_value.as_ref()),
            Some(new_value.as_ref()),
        )??;
    }

    #[fehler::throws]
    fn remove(&self, collection: impl AsRef<[u8]>, key: impl AsRef<[u8]>) {
        let tree = self.open_tree(collection)?;

        tree.remove(key.as_ref())?;
    }

    #[fehler::throws]
    fn exists(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> bool {
        let tree = self.open_tree(collection)?;

        tree.contains_key(key)?
    }

    async fn flush(&self) -> Result<usize, Error> {
        self.flush_async().await.map_err(Error::from)
    }
}
