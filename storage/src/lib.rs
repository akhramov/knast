mod sled_engine;

use std::path::{Path, PathBuf};

use anyhow::Error;
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

#[async_trait]
pub trait StorageEngine {
    fn initialize(cache_dir: impl AsRef<Path>) -> Result<Box<Self>, Error>;

    fn get(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> Result<Option<Vec<u8>>, Error>;

    fn put(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) -> Result<(), Error>;

    fn exists(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> Result<bool, Error>;

    async fn flush(&self) -> Result<usize, Error>;
}

pub type SledStorage = Storage<sled::Db>;

pub struct Storage<T: StorageEngine> {
    inner: Box<T>,
    cache_dir: PathBuf,
}

impl<T: StorageEngine> Storage<T> {
    #[fehler::throws]
    pub fn new(cache_dir: impl AsRef<Path>) -> Self {
        Self {
            cache_dir: cache_dir.as_ref().into(),
            inner: T::initialize(cache_dir)?,
        }
    }

    #[fehler::throws]
    pub fn get<D: DeserializeOwned>(
        &self,
        store: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> Option<D> {
        self.inner.get(store, key)?
            .map(|value| bincode::deserialize(&value))
            .transpose()?
    }

    #[fehler::throws]
    pub fn put<S: Serialize>(
        &self,
        store: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        value: S,
    ) -> S {
        let serialized_value = bincode::serialize(&value)?;

        self.inner.put(store, key, serialized_value)?;

        value
    }

    #[fehler::throws]
    pub fn exists(
        &self,
        store: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> bool {
        self.inner.exists(store, key)?
    }

    pub async fn flush(&self) -> Result<usize, Error> {
        self.inner.flush().await
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
            Storage::<sled::Db>::new(dir.path()).expect("Unable to initialize cache");

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
