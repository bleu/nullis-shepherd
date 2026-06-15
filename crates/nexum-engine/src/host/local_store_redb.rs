//! `nexum:host/local-store` backend.
//!
//! Single redb file under `EngineConfig.engine.state_dir`. Per-module
//! namespacing is enforced host-side via a fixed-length 32-byte prefix:
//! `keccak256(module_name) ++ raw_key`. Two modules using the same key
//! string see disjoint data regardless of how similar their names are.
//!
//! The 32-byte hash prefix has two properties that the old
//! `[len:u8][name][key]` scheme lacked:
//!
//! - **Fixed width** - no length field to forge; a module cannot craft a
//!   key that bleeds into another module's prefix range.
//! - **ENS-compatible** - keccak256 is the same hash used by ENS node
//!   derivation, so module identities can be derived from ENS names
//!   without extra hashing in the future (ADR-0003).
//!
//! ## Per-module handle
//!
//! [`LocalStore`] is the process-wide handle (one redb file shared across
//! every module). [`ModuleStore`] is the per-module view: it carries the
//! pre-computed namespace prefix once, so every get / set / delete /
//! list-keys call concatenates `prefix ++ key` without re-hashing the
//! module name. The supervisor builds one `ModuleStore` per module at
//! instantiation via [`LocalStore::module`].

#![allow(clippy::result_large_err)]

use std::path::Path;
use std::sync::Arc;

use alloy_primitives::keccak256;
use redb::{Database, TableDefinition};
use thiserror::Error;

const TABLE: TableDefinition<'static, &[u8], &[u8]> = TableDefinition::new("nexum:local-store");
const PREFIX_LEN: usize = 32;

/// Process-wide handle to the local-store redb database. Cheap to clone;
/// per-module access is created via [`LocalStore::module`].
#[derive(Debug, Clone)]
pub struct LocalStore {
    db: Arc<Database>,
}

impl LocalStore {
    /// Open (or create) the redb file at `path`. Materialises the shared
    /// table so subsequent read transactions never hit `TableDoesNotExist`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let db = Database::create(path).map_err(StorageError::Open)?;
        {
            let txn = db.begin_write().map_err(StorageError::Txn)?;
            txn.open_table(TABLE).map_err(StorageError::Table)?;
            txn.commit().map_err(StorageError::Commit)?;
        }
        Ok(Self { db: Arc::new(db) })
    }

    /// Build a per-module view with the namespace prefix computed once.
    ///
    /// Returns [`StorageError::InvalidNamespace`] for the empty string so
    /// callers can rely on a non-trivial prefix. The returned
    /// [`ModuleStore`] shares the same `Arc<Database>` as `self`; cloning
    /// it is cheap (Arc bump + 32 bytes).
    pub fn module(&self, namespace: &str) -> Result<ModuleStore, StorageError> {
        if namespace.is_empty() {
            return Err(StorageError::InvalidNamespace(
                "module namespace must not be empty".into(),
            ));
        }
        let prefix = keccak256(namespace.as_bytes()).0;
        Ok(ModuleStore {
            db: Arc::clone(&self.db),
            prefix,
        })
    }
}

/// Per-module view of the local store. Carries the precomputed
/// `keccak256(module_name)` prefix so every operation concatenates
/// `prefix ++ key` without re-hashing the name. Cheap to clone (Arc
/// bump + 32 bytes).
#[derive(Debug, Clone)]
pub struct ModuleStore {
    db: Arc<Database>,
    prefix: [u8; PREFIX_LEN],
}

impl ModuleStore {
    /// Fetch a value for `key`. Returns `Ok(None)` when no entry exists;
    /// the module never observes the prefix.
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let full = self.build_key(key);
        let txn = self.db.begin_read().map_err(StorageError::Txn)?;
        let table = txn.open_table(TABLE).map_err(StorageError::Table)?;
        let value = table
            .get(full.as_slice())
            .map_err(StorageError::Storage)?
            .map(|v| v.value().to_vec());
        Ok(value)
    }

    /// Insert or overwrite.
    pub fn set(&self, key: &str, value: &[u8]) -> Result<(), StorageError> {
        let full = self.build_key(key);
        let txn = self.db.begin_write().map_err(StorageError::Txn)?;
        {
            let mut table = txn.open_table(TABLE).map_err(StorageError::Table)?;
            table
                .insert(full.as_slice(), value)
                .map_err(StorageError::Storage)?;
        }
        txn.commit().map_err(StorageError::Commit)?;
        Ok(())
    }

    /// Delete. Idempotent - deleting a missing key is a no-op.
    pub fn delete(&self, key: &str) -> Result<(), StorageError> {
        let full = self.build_key(key);
        let txn = self.db.begin_write().map_err(StorageError::Txn)?;
        {
            let mut table = txn.open_table(TABLE).map_err(StorageError::Table)?;
            table
                .remove(full.as_slice())
                .map_err(StorageError::Storage)?;
        }
        txn.commit().map_err(StorageError::Commit)?;
        Ok(())
    }

    /// Enumerate keys whose raw key (post-prefix) starts with `prefix`.
    /// Returns only the module-visible key strings - the host strips
    /// the namespace prefix.
    pub fn list_keys(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let full_prefix = self.build_key(prefix);
        let txn = self.db.begin_read().map_err(StorageError::Txn)?;
        let table = txn.open_table(TABLE).map_err(StorageError::Table)?;
        let mut out = Vec::new();
        // redb's B-tree iterates keys in sorted order, so a range
        // starting at `full_prefix` only touches matching entries (and
        // the first key past the prefix range). Breaking on the first
        // non-matching key keeps this O(matching entries) instead of
        // the O(total DB entries) `table.iter()` would do.
        for entry in table
            .range(full_prefix.as_slice()..)
            .map_err(StorageError::Storage)?
        {
            let (k, _v) = entry.map_err(StorageError::Storage)?;
            let key_bytes = k.value();
            if !key_bytes.starts_with(&full_prefix) {
                break;
            }
            if let Ok(s) = std::str::from_utf8(&key_bytes[PREFIX_LEN..]) {
                out.push(s.to_owned());
            }
        }
        Ok(out)
    }

    fn build_key(&self, key: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(PREFIX_LEN + key.len());
        out.extend_from_slice(&self.prefix);
        out.extend_from_slice(key.as_bytes());
        out
    }
}

/// Errors surfaced by [`LocalStore`] / [`ModuleStore`].
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("open redb: {0}")]
    Open(#[source] redb::DatabaseError),
    #[error("redb txn: {0}")]
    Txn(#[source] redb::TransactionError),
    #[error("redb table: {0}")]
    Table(#[source] redb::TableError),
    #[error("redb storage: {0}")]
    Storage(#[source] redb::StorageError),
    #[error("redb commit: {0}")]
    Commit(#[source] redb::CommitError),
    #[error("invalid namespace: {0}")]
    InvalidNamespace(String),
}

#[cfg(test)]
mod tests;
