#![forbid(unsafe_code)]
//! Core traits for the storeit-rs repository library.
//! This crate is database-agnostic and should not contain any backend-specific logic.

// Re-export for downstream macro expansions (used by storeit_macros::repository)
pub use async_trait::async_trait;

// Public transactions module (backend-agnostic abstractions)
pub mod transactions;

/// Marker trait for types that can be fetched from a database.
/// Implemented via `#[derive(Fetchable)]` proc-macro in `storeit_macros`.
///
/// Provides compile-time metadata used by repository generators.
pub trait Fetchable {
    const TABLE: &'static str;
    const SELECT_COLUMNS: &'static [&'static str];

    /// A list of (column_name, rust_type) tuples for fields that can be used
    /// to generate `find_by...` methods.
    const FINDABLE_COLUMNS: &'static [(&'static str, &'static str)];
}

/// A backend-agnostic representation of a database parameter value.
/// This is used to pass entity field values from generated code to backend adapters
/// without making `storeit_core` dependent on a specific database driver.
#[derive(Debug, Clone)]
pub enum ParamValue {
    String(String),
    I32(i32),
    I64(i64),
    F64(f64),
    Bool(bool),
    Null,
}

/// Trait for entities that have an identifiable key.
/// This trait exposes the key type and column name so macros can introspect it.
pub trait Identifiable {
    /// The type of the primary key (e.g., `i64`, `Uuid`).
    type Key;

    /// The name of the primary key column in the database.
    const ID_COLUMN: &'static str;

    /// Returns a copy of the entity's ID, if it has one.
    fn id(&self) -> Option<Self::Key>;
}

/// Trait for types whose fields can be extracted for an INSERT statement.
/// This is implemented by the `#[derive(Fetchable)]` macro.
pub trait Insertable {
    /// The columns to be used in an INSERT statement, excluding auto-generated keys.
    const INSERT_COLUMNS: &'static [&'static str];

    /// The values of the fields corresponding to `INSERT_COLUMNS`.
    fn insert_values(&self) -> Vec<ParamValue>;
}

/// Trait for types whose fields can be extracted for an UPDATE statement.
/// This is implemented by the `#[derive(Fetchable)]` macro.
pub trait Updatable {
    /// The columns to be used in an UPDATE statement's SET clause.
    const UPDATE_COLUMNS: &'static [&'static str];

    /// The values of the fields corresponding to `UPDATE_COLUMNS`.
    fn update_values(&self) -> Vec<ParamValue>;
}

/// Lightweight, backend-agnostic error type for repository operations.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// The entity was not found.
    #[error("entity not found")]
    NotFound,
    /// Error while mapping a backend row into an entity.
    #[error("mapping error")]
    Mapping {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Opaque backend error from the underlying driver or adapter.
    #[error("backend error")]
    Backend {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl RepoError {
    /// Wrap a backend/driver error.
    pub fn backend<E>(e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        RepoError::Backend {
            source: Box::new(e),
        }
    }
    /// Wrap a row-mapping error.
    pub fn mapping<E>(e: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        RepoError::Mapping {
            source: Box::new(e),
        }
    }
}

/// Convenience alias for results returned by repository methods.
pub type RepoResult<T> = Result<T, RepoError>;

/// A trait describing a minimal, asynchronous repository interface for an entity `T` identified by key `K`.
/// This is intentionally DB-agnostic. Concrete backends provide implementations.
#[async_trait]
pub trait Repository<T: Identifiable> {
    /// Fetch an entity by its primary key. Returns Ok(None) if not found.
    async fn find_by_id(&self, id: &T::Key) -> RepoResult<Option<T>>;

    /// A generic finder for a single field. Returns a (possibly empty) Vec of entities.
    /// This is the low-level hook used by macro-generated `find_by_<field>` methods.
    async fn find_by_field(&self, field_name: &str, value: ParamValue) -> RepoResult<Vec<T>>;

    /// Insert a new entity. The returned entity may be different if the database
    /// generates some fields (e.g., auto-incrementing IDs).
    async fn insert(&self, entity: &T) -> RepoResult<T>;

    /// Update an existing entity. The returned entity may be different if the
    /// database modifies it (e.g., `ON UPDATE` timestamps).
    async fn update(&self, entity: &T) -> RepoResult<T>;

    /// Delete an entity by key. Returns true if a row was affected.
    async fn delete_by_id(&self, id: &T::Key) -> RepoResult<bool>;
}

/// A tiny adapter for mapping a backend-specific row type into an entity `T`.
/// Backends (e.g., rusqlite, mysql_async, tokio_postgres) can implement this for their row representations.
#[allow(clippy::wrong_self_convention)]
pub trait RowAdapter<T> {
    type Row;
    fn from_row(&self, row: &Self::Row) -> RepoResult<T>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_error_display_messages() {
        let e1 = RepoError::NotFound;
        assert_eq!(format!("{}", e1), "entity not found");

        let e2 = RepoError::mapping(std::io::Error::new(std::io::ErrorKind::Other, "bad row"));
        // Display should start with the variant message; source included by Debug chain.
        assert_eq!(format!("{}", e2), "mapping error");

        let e3 = RepoError::backend(std::io::Error::new(std::io::ErrorKind::Other, "boom"));
        assert_eq!(format!("{}", e3), "backend error");
    }

    #[test]
    fn param_value_variants_roundtrip() {
        // Construct all variants and ensure pattern matching reads expected values
        let values = vec![
            ParamValue::String("s".to_string()),
            ParamValue::I32(32),
            ParamValue::I64(64),
            ParamValue::F64(6.5),
            ParamValue::Bool(true),
            ParamValue::Null,
        ];

        for v in values {
            match v.clone() {
                ParamValue::String(s) => assert_eq!(s, "s"),
                ParamValue::I32(i) => assert_eq!(i, 32),
                ParamValue::I64(i) => assert_eq!(i, 64),
                ParamValue::F64(f) => assert_eq!(f, 6.5),
                ParamValue::Bool(b) => assert!(b),
                ParamValue::Null => assert!(matches!(v, ParamValue::Null)),
            }
        }
    }

    // A tiny entity and RowAdapter example to exercise trait wiring
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MiniEntity {
        id: Option<i64>,
    }

    impl Identifiable for MiniEntity {
        type Key = i64;
        const ID_COLUMN: &'static str = "id";
        fn id(&self) -> Option<Self::Key> {
            self.id
        }
    }

    struct MiniAdapter;
    impl RowAdapter<MiniEntity> for MiniAdapter {
        type Row = i64; // pretend a row is just an i64 id
        fn from_row(&self, row: &Self::Row) -> Result<MiniEntity, RepoError> {
            Ok(MiniEntity { id: Some(*row) })
        }
    }

    #[test]
    fn row_adapter_from_row_works() {
        let a = MiniAdapter;
        let ent = a.from_row(&7).unwrap();
        assert_eq!(ent, MiniEntity { id: Some(7) });
    }

    // Simple function using RepoResult to ensure alias is exercised
    fn maybe_ok(flag: bool) -> RepoResult<i32> {
        if flag {
            Ok(1)
        } else {
            Err(RepoError::NotFound)
        }
    }

    #[test]
    fn repo_result_alias_usage() {
        assert_eq!(maybe_ok(true).unwrap(), 1);
        let err = maybe_ok(false).unwrap_err();
        matches!(err, RepoError::NotFound);
    }
}
