#![forbid(unsafe_code)]
//! Facade crate re-exporting core traits and macros for the `storeit-rs` library.
//!
//! This crate provides the main public API. It re-exports all necessary traits
//! and procedural macros so that you only need to add this single crate as a
//! dependency in your application.
//!
//! # Example: Deriving `Entity`
//!
//! The `#[derive(Entity)]` macro generates compile-time metadata about your entity,
//! including a default `RowAdapter` implementation.
//! ```ignore
//! // This is a non-runnable snippet to avoid pulling backend-specific adapter impls
//! // into doctest scope. See the runnable examples under `storeit/examples/`.
//! use storeit::{Entity, Fetchable}; // `Fetchable` must be in scope to access associated constants.
//!
//! // The macro automatically deduces the table name (`users`) by pluralizing
//! // the snake_case version of the struct name.
//! #[derive(Entity, Clone, Debug)]
//! pub struct User {
//!     // Mark the ID field. The key type (`i64`) and column name ("id") are deduced.
//!     #[fetch(id)]
//!     pub id: Option<i64>,
//!     // You can override column names.
//!     #[fetch(column = "email_address")]
//!     pub email: String,
//! }
//!
//! // The `Entity` derive generates metadata constants via the `Fetchable` trait:
//! assert_eq!(User::TABLE, "users");
//! assert_eq!(User::SELECT_COLUMNS, &["id", "email_address"]);
//!
//! // It also generates a `RowAdapter` struct named `UserRowAdapter`.
//! // This line is a compile-time check that the struct was created.
//! let _adapter = UserRowAdapter;
//! ```
//!
//! # Example: Generating a Repository
//!
//! A runnable example showcasing the `#[repository]` macro can be found in the
//! `/examples` directory of the workspace. It is more comprehensive than a doctest
//! can be, as it involves multiple crates and backend-specific features.

#![allow(unexpected_cfgs)] // Temporarily allow unknown `cfg(feature = "dep:*")` values until backend crates are published

// Re-export all core traits.
pub use storeit_core::{
    Fetchable, Identifiable, Insertable, ParamValue, RepoError, RepoResult, Repository, RowAdapter,
    Updatable,
};

// Re-export all procedural macros.
pub use storeit_macros::{repository, Entity};

// Optional re-export of the SQL builder helpers.
#[cfg(feature = "sql-builder")]
pub use storeit_sql_builder as sql_builder;

// Re-export backend-agnostic transactions API so end-users can import from `repository`.
pub use storeit_core::transactions;

// Optional query ergonomics: simple paginate helper in-memory.
#[cfg(feature = "query-ext")]
pub mod query_ext {
    use crate::RepoResult;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Page<T> {
        pub items: Vec<T>,
        pub total: usize,
    }

    #[async_trait::async_trait]
    pub trait RepositoryExt<T>: storeit_core::Repository<T>
    where
        T: storeit_core::Identifiable + Send + Sync + 'static,
    {
        /// Naive pagination over an equality filter. This method fetches all matching rows
        /// using `find_by_field` and then slices in-memory. Intended for quick prototyping.
        /// For efficient, large-scale pagination, implement a backend-specific method that uses
        /// LIMIT/OFFSET or keyset and a COUNT query.
        async fn paginate_by_field(
            &self,
            field_name: &str,
            value: storeit_core::ParamValue,
            page: usize,
            size: usize,
        ) -> RepoResult<Page<T>> {
            let all = self.find_by_field(field_name, value).await?;
            let total = all.len();
            let start = page.saturating_mul(size);
            if start >= total {
                return Ok(Page {
                    items: Vec::new(),
                    total,
                });
            }
            let end = usize::min(start + size, total);
            let items = all.into_iter().skip(start).take(end - start).collect();
            Ok(Page { items, total })
        }
    }

    #[async_trait::async_trait]
    impl<T, R> RepositoryExt<T> for R
    where
        T: storeit_core::Identifiable + Send + Sync + 'static,
        R: storeit_core::Repository<T> + Send + Sync,
    {
    }
}

// Optional batch insert extension: naive loop-based insert_many.
#[cfg(feature = "batch-ext")]
pub mod batch_ext {
    use crate::RepoResult;
    #[async_trait::async_trait]
    pub trait BatchInsertExt<T>: storeit_core::Repository<T>
    where
        T: storeit_core::Identifiable + Send + Sync + Clone + 'static,
    {
        /// Insert many entities in sequence. This is a naive fallback implementation
        /// intended for convenience and prototyping. Backends may add efficient
        /// multi-row VALUES support in the future.
        async fn insert_many(&self, entities: &[T]) -> RepoResult<Vec<T>> {
            let mut out = Vec::with_capacity(entities.len());
            for e in entities {
                out.push(self.insert(e).await?);
            }
            Ok(out)
        }
    }

    #[async_trait::async_trait]
    impl<T, R> BatchInsertExt<T> for R
    where
        T: storeit_core::Identifiable + Send + Sync + Clone + 'static,
        R: storeit_core::Repository<T> + Send + Sync,
    {
    }
}

// Optional streaming extension: wraps find_by_field() into a Stream.
#[cfg(feature = "stream-ext")]
pub mod stream_ext {
    use crate::RepoResult;
    use futures_core::Stream;
    use std::pin::Pin;

    pub trait FindStreamExt<T>: storeit_core::Repository<T>
    where
        T: storeit_core::Identifiable + Send + Sync + 'static,
    {
        /// Returns a stream over the results of `find_by_field`. This is a simple wrapper
        /// that fetches all data and yields items one by one; it is intended to provide
        /// a streaming shape without requiring backend-specific APIs yet.
        fn find_by_field_stream(
            &self,
            field_name: &str,
            value: storeit_core::ParamValue,
        ) -> Pin<Box<dyn Stream<Item = RepoResult<T>> + Send + '_>>;
    }

    impl<T, R> FindStreamExt<T> for R
    where
        T: storeit_core::Identifiable + Send + Sync + 'static,
        R: storeit_core::Repository<T> + Send + Sync,
    {
        fn find_by_field_stream(
            &self,
            field_name: &str,
            value: storeit_core::ParamValue,
        ) -> Pin<Box<dyn Stream<Item = RepoResult<T>> + Send + '_>> {
            let field = field_name.to_string();
            let this = self;
            Box::pin(async_stream::try_stream! {
                let items = this.find_by_field(&field, value).await?;
                for item in items {
                    yield item;
                }
            })
        }
    }
}

// Backend repositories re-exported under a neutral namespace, so end-users don't
// have to depend on backend crates directly. These are feature-gated.
#[cfg(feature = "upsert-ext")]
pub mod upsert_ext {
    use crate::RepoResult;
    #[async_trait::async_trait]
    pub trait UpsertExt<T>: storeit_core::Repository<T>
    where
        T: storeit_core::Identifiable
            + storeit_core::Insertable
            + storeit_core::Updatable
            + Send
            + Sync
            + Clone
            + 'static,
    {
        /// Naive upsert by primary key: attempt insert, and on error fall back to update.
        /// This is a portability helper meant for prototyping. Backends may provide more
        /// efficient, conflict-targeted upserts.
        async fn upsert_by_id(&self, entity: &T) -> RepoResult<T> {
            match self.insert(entity).await {
                Ok(v) => Ok(v),
                Err(_e) => {
                    // On any insert error, try update as a best-effort fallback.
                    self.update(entity).await
                }
            }
        }
    }

    #[async_trait::async_trait]
    impl<T, R> UpsertExt<T> for R
    where
        T: storeit_core::Identifiable
            + storeit_core::Insertable
            + storeit_core::Updatable
            + Send
            + Sync
            + Clone
            + 'static,
        R: storeit_core::Repository<T> + Send + Sync,
    {
    }
}

pub mod backends {
    #[cfg(feature = "libsql-backend")]
    pub use storeit_libsql::{LibsqlRepository, LibsqlTransactionManager};
    // Only re-export MySQL backend types when the optional dependency exists and is enabled.
    // This avoids unresolved imports during CI --all-features when the dependency is temporarily
    // removed from Cargo.toml for initial publishing.
    #[cfg(feature = "dep:storeit_mysql_async")]
    pub use storeit_mysql_async::{MysqlAsyncRepository, MysqlAsyncTransactionManager};
    // Same for Postgres backend types.
    #[cfg(feature = "dep:storeit_tokio_postgres")]
    pub use storeit_tokio_postgres::{TokioPostgresRepository, TokioPostgresTransactionManager};
}
