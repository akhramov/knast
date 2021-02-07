pub const BLOBS_STORAGE_KEY: &[u8] = b"blobs";
pub const IMAGES_INDEX_STORAGE_KEY: &[u8] = b"images";

pub use storage::Storage;
pub use storage::StorageEngine;

#[cfg(test)]
pub use storage::SledStorage as TestStorage;
