use std::{future::Future, path::Path};

use anyhow::Error;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::named_params;

use super::StorageEngine;

const STORAGE_FILE: &str = "storage.db";

pub type Connection = Pool<SqliteConnectionManager>;

impl StorageEngine for Connection {
    #[fehler::throws]
    fn initialize(cache_dir: impl AsRef<Path>) -> Box<Self> {
        let file = cache_dir.as_ref().join(STORAGE_FILE);
        let manager = SqliteConnectionManager::file(file);
        let pool = r2d2::Pool::new(manager)?;
        let connection = pool.get()?;
        connection.execute(include_str!("sqlite_engine/migration.sql"), [])?;

        Box::new(pool)
    }

    fn get(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> Result<Option<Vec<u8>>, Error> {
        let connection = self.get()?;
        let mut get_statement = connection
            .prepare_cached(include_str!("sqlite_engine/get.sql"))?;

        let params = named_params! {
            ":key": key.as_ref(),
            ":tree": collection.as_ref()
        };
        let mut results = get_statement.query_map(params, |row| {
            let result: Vec<u8> = row.get(0)?;

            Ok(result)
        })?;

        results.next().transpose().map_err(From::from)
    }

    #[fehler::throws]
    fn put(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        value: impl AsRef<[u8]>,
    ) {
        let connection = self.get()?;
        let mut put_statement = connection
            .prepare_cached(include_str!("sqlite_engine/put.sql"))?;

        let params = named_params! {
            ":key": key.as_ref(),
            ":tree": collection.as_ref(),
            ":value": value.as_ref(),
        };
        put_statement.execute(params)?;
    }

    #[fehler::throws]
    fn compare_and_swap(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
        old_value: Option<impl AsRef<[u8]>>,
        new_value: Option<impl AsRef<[u8]>>,
    ) {
        let mut connection = self.get()?;
        let old_value = if let Some(old_value) = &old_value {
            Some(old_value.as_ref())
        } else {
            None
        };
        let new_value = if let Some(new_value) = &new_value {
            Some(new_value.as_ref())
        } else {
            None
        };

        let tx = connection.transaction()?;
        {
            let insert_params = named_params! {
                ":key": key.as_ref(),
                ":tree": collection.as_ref(),
                ":old_value": old_value,
            };
            let mut try_insert_statement = tx.prepare_cached(include_str!(
                "sqlite_engine/try_insert.sql"
            ))?;

            try_insert_statement.execute(insert_params)?;
        }

        {
            let cas_params = named_params! {
                ":key": key.as_ref(),
                ":tree": collection.as_ref(),
                ":old_value": old_value,
                ":new_value": new_value,
            };

            let mut cas_statement =
                tx.prepare_cached(include_str!("sqlite_engine/cas.sql"))?;
            let mut rows = cas_statement.query(cas_params)?;

            if rows.next()?.is_none() {
                anyhow::bail!("Compare and swap conflict");
            }
        }

        tx.commit()?;
    }

    #[fehler::throws]
    fn remove(&self, collection: impl AsRef<[u8]>, key: impl AsRef<[u8]>) {
        let connection = self.get()?;
        let mut remove_statement = connection
            .prepare_cached(include_str!("sqlite_engine/remove.sql"))?;
        let params = named_params! {
            ":key": key.as_ref(),
            ":tree": collection.as_ref(),
        };

        remove_statement.execute(params)?;
    }

    #[fehler::throws]
    fn exists(
        &self,
        collection: impl AsRef<[u8]>,
        key: impl AsRef<[u8]>,
    ) -> bool {
        let connection = self.get()?;
        let mut exists_statement = connection
            .prepare_cached(include_str!("sqlite_engine/exists.sql"))?;
        let params = named_params! {
            ":key": key.as_ref(),
            ":tree": collection.as_ref(),
        };

        let mut results = exists_statement.query_map(params, |row| {
            let result: bool = row.get(0)?;

            Ok(result)
        })?;

        results.next().transpose()?.unwrap_or_default()
    }

    fn flush(&self) -> Box<dyn Future<Output = Result<usize, Error>> + Unpin> {
        Box::new(std::future::ready(Ok(0)))
    }
}
