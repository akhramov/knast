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

    fn compare_and_swap(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        old_value: Option<impl AsRef<[u8]>>,
        new_value: Option<impl AsRef<[u8]>>,
    ) -> Result<(), Error>;

    fn remove(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
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
        self.inner
            .get(store, key)?
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
    pub fn compare_and_swap<S: Serialize>(
        &self,
        store: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        old_value: Option<S>,
        new_value: Option<S>,
    ) -> Option<S> {
        let serialized_old_value = if let Some(old_value) = &old_value {
            Some(bincode::serialize(&old_value)?)
        } else {
            None
        };
        let serialized_new_value = if let Some(new_value) = &new_value {
            Some(bincode::serialize(&new_value)?)
        } else {
            None
        };

        self.inner.compare_and_swap(
            store,
            key,
            serialized_old_value,
            serialized_new_value,
        )?;

        new_value
    }

    #[fehler::throws]
    pub fn remove(&self, store: impl AsRef<[u8]>, key: impl AsRef<[u8]>) {
        self.inner.remove(store, key)?;
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

        let cache = Storage::<sled::Db>::new(dir.path())
            .expect("Unable to initialize cache");

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

    #[test]
    fn test_compare_and_swap() {
        let dir =
            tempfile::tempdir().expect("failed to create a tmp directory");

        let cache = Storage::<sled::Db>::new(dir.path())
            .expect("Unable to initialize cache");

        let value: Vec<u8> = b"ipsum"[..].into();
        let new_value: Vec<u8> = b"dolor"[..].into();
        let tree = b"test";
        let key = b"lorem";

        // Put the value into the tree.
        cache
            .put(tree, key, &value)
            .expect("Failed to put a value into the cache");
        // Cas #1: swap the old value with the new one
        cache
            .compare_and_swap(tree, key, Some(&value), Some(&new_value))
            .expect("CAS failed unexpectedly");

        let stored_value: Vec<u8> = cache.get(tree, key).unwrap().unwrap();
        assert_eq!(stored_value, new_value);
        // Cas #2: swap the old value back
        cache
            .compare_and_swap(tree, key, Some(&new_value), Some(&value))
            .expect("CAS failed unexpectedly");

        let stored_value: Vec<u8> = cache.get(tree, key).unwrap().unwrap();
        assert_eq!(stored_value, value);

        // Cas #3: attempt invalid swap
        let err = cache
            .compare_and_swap(tree, key, Some(&new_value), Some(&value))
            .unwrap_err();

        assert!(err.to_string().contains("Compare and swap conflict"));
    }

    #[test]
    fn test_remove() {
        let dir =
            tempfile::tempdir().expect("failed to create a tmp directory");

        let cache = Storage::<sled::Db>::new(dir.path())
            .expect("Unable to initialize cache");

        let value: Vec<u8> = b"ipsum"[..].into();
        let tree = b"test";
        let key = b"lorem";

        // Put the value into the tree.
        cache
            .put(tree, key, &value)
            .expect("Failed to put a value into the cache");

        cache
            .remove(tree, key)
            .expect("Failed to remove a value from the cache");

        let stored_value: Option<Vec<u8>> = cache.get(tree, key).unwrap();
        assert_eq!(stored_value, None);
    }
}
