use rocksdb::Error;

/// An error from the storage layer
pub enum StorageError {
    RocksDbError(rocksdb::Error),
}

impl From<rocksdb::Error> for StorageError {
    fn from(err: Error) -> Self {
        StorageError::RocksDbError(err)
    }
}