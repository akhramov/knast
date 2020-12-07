use std::path::{Path, PathBuf};

use anyhow::Error;
use serde::{de::DeserializeOwned, Serialize};

const STORAGE_FILE: &str = "storage.db";

pub const IMAGES_INDEX_STORAGE_KEY: &[u8] = b"images";
pub const BLOBS_STORAGE_KEY: &[u8] = b"blobs";

pub struct Storage {
    inner: sled::Db,
    cache_dir: PathBuf,
}

impl Storage {
    #[fehler::throws]
    pub fn new(cache_dir: impl AsRef<Path>) -> Self {
        Self {
            cache_dir: cache_dir.as_ref().into(),
            inner: sled::open(cache_dir.as_ref().join(STORAGE_FILE))?,
        }
    }

    #[fehler::throws]
    pub fn get<T: DeserializeOwned>(
        &self,
        store: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> Option<T> {
        let tree = self.inner.open_tree(store)?;

        tree.get(key)?
            .map(|value| bincode::deserialize(&value))
            .transpose()?
    }

    #[fehler::throws]
    pub fn put<T: Serialize>(
        &self,
        store: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        value: T,
    ) -> T {
        let tree = self.inner.open_tree(store)?;
        let serialized_value = bincode::serialize(&value)?;

        tree.insert(key.as_ref(), serialized_value)?;

        value
    }

    #[fehler::throws]
    pub fn exists(
        &self,
        store: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> bool {
        let tree = self.inner.open_tree(store)?;

        tree.contains_key(key)?
    }

    #[fehler::throws]
    pub async fn flush(&self) -> usize {
        self.inner.flush_async().await?
    }

    pub fn folder(&self) -> PathBuf {
        self.cache_dir.clone()
    }
}

#[cfg(test)]
mod test {
    use super::Storage;

    #[test]
    fn test_happy_path() {
        let dir =
            tempfile::tempdir().expect("failed to create a tmp directory");

        let cache =
            Storage::new(dir.path()).expect("Unable to initialize cache");

        let value: Vec<u8> = b"ipsum"[..].into();
        let tree = b"test";
        let key = b"lorem";

        cache
            .put(tree, key, &value)
            .expect("Failed to put a value into the cache");

        let stored_value: Vec<u8> = cache.get(tree, key).unwrap().unwrap();

        assert_eq!(stored_value, value);
        assert_eq!(cache.folder(), dir.path());
        assert!(cache.exists(tree, key).unwrap())
    }
}
