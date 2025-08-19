# storeit-rs

Generate Spring Data–style SQL repositories with Rust proc-macros.

Workspace crates:
- storeit_core: core traits/helpers
- storeit_macros: proc-macros (derive/attributes)
- storeit: facade re-exporting both (examples may alias this as `repository` via Cargo dependency renaming)
- storeit_sql_builder: SQL string builders using Fetchable metadata
- storeit_libsql: SQLite-family backend using libsql (feature-gated, not a workspace member by default)
- storeit_mysql_async: MySQL backend (feature-gated, not a workspace member by default)
- storeit_tokio_postgres: Postgres backend (feature-gated, not a workspace member by default)

## Current state
- Core traits exist and are backend-agnostic: `Fetchable`, `Identifiable`, `Insertable`, `Updatable`, `RowAdapter<T>`, plus lightweight error types `RepoError`/`RepoResult`.
- The asynchronous `Repository<T>` trait (with `T: Identifiable`) defines: `find_by_id`, `find_by_field`, `insert`, `update`, and `delete_by_id`.
- `#[derive(Entity)]` macro auto-generates compile-time metadata (`TABLE`, `SELECT_COLUMNS`, `FINDABLE_COLUMNS`), implements `Identifiable`, `Insertable`, and `Updatable`, and also generates a backend-specific `RowAdapter` type (feature-gated per backend). Per-field overrides via `#[fetch(column = "...")]`, ID via `#[fetch(id)]`, and optional table override via `#[entity(table = "...")]`.
- `#[repository(entity = ..., backend = ..., finders(...))]` attribute macro generates a thin typed wrapper around a chosen backend repository and synthesizes inherent `find_by_<field>` methods that delegate to the backend via `find_by_field`.
- SQL builder helpers in `storeit_sql_builder` generate SQL strings (SELECT/INSERT/UPDATE/DELETE and pagination). Placeholder styles are selected via features. Includes unit tests.

## Feature matrix (by backend)
- Core (DB-agnostic):
  - Traits: `Fetchable`, `Identifiable`, `Insertable`, `Updatable`, `RowAdapter<T>`, async `Repository<T>`. Implemented and used by all adapters. ✓
  - Macros: `#[derive(Entity)]` and `#[repository(...)]`. Generate metadata, adapter-friendly value extraction, and a typed repository wrapper with derived `find_by_*` methods. ✓
  - Examples and doc tests exist in the `storeit` facade (aliased as `repository` in examples via Cargo dependency renaming). ✓
- SQL builder (storeit_sql_builder):
  - Implemented helpers using `Fetchable` metadata: `select_by_id`, `delete_by_id`, `insert`, `update_by_id`, `select_all`, `select_by_field`, `select_with_pagination`. Each has unit tests. ✓
  - Placeholder style via features: `tokio_postgres` -> `$1,$2,...`; others -> `?` as default. ✓
  - Note: Builders emit strings only; they don’t execute queries. ✓
- SQLite backend (storeit_libsql):
  - Adapter `LibsqlRepository<T, A>` using libsql. Fully async. Implements `find_by_id`, `find_by_field`, `insert`, `update`, `delete_by_id`. ✓
- mysql_async backend (storeit_mysql_async):
  - Adapter `MysqlAsyncRepository<T, A>` using a mysql_async Pool and a user-provided RowAdapter. Implements `find_by_id`, `find_by_field`, `insert`, `update`, `delete_by_id`. ✓
- tokio_postgres backend (storeit_tokio_postgres):
  - Adapter `TokioPostgresRepository<T, A>` that manages a tokio_postgres client and a background connection task. Implements `find_by_id`, `find_by_field`, `insert`, `update`, `delete_by_id`. ✓

Backend coverage summary:
- Postgres (tokio_postgres): full basic CRUD + generic find_by_field via ParamValue. ✓
- SQLite (libsql): full basic CRUD + generic find_by_field via ParamValue. ✓
- MySQL (mysql_async): full basic CRUD + generic find_by_field via ParamValue. ✓
- Other backends: none at the moment.

## Goal (high level)
- Provide macros that autogenerate common query methods (e.g., `find_by_id`, `find_all`, derived `find_by_<field>`), plus glue for popular SQL backends.

## Quickstart: End-user code

Canonical pattern: create repositories once (singletons) and reuse them inside TransactionTemplate; this mirrors docs/architecture.md.

Note on crate names and aliasing: The facade crate is named `storeit`. Examples below use `repository::...` paths by aliasing the dependency in Cargo.toml:

```toml
[dependencies]
# Local path usage during development
repository = { package = "storeit", path = "./storeit" }
# Or, from crates.io (when published)
# repository = { package = "storeit", version = "0.1" }
```

Transactions (backend-agnostic): See also docs/architecture.md (transactions section) and docs/transactions.md.
A minimal, backend-agnostic API is now available via `repository::transactions` (re-exported from `storeit_core`). Backends will provide concrete `TransactionManager` implementations later, but end-user code can already be written against the generic `TransactionTemplate<M>` and `TransactionManager` trait. Example skeleton:

```rust
use repository::transactions::{TransactionTemplate, TransactionManager, TransactionDefinition, TransactionContext};

// Your application can be generic over `M: TransactionManager`.
async fn do_work<M: TransactionManager>(tpl: &TransactionTemplate<M>) -> repository::RepoResult<i32> {
    tpl.execute(|_ctx: TransactionContext| async move {
        // Obtain tx-bound repositories from the context using your chosen backend's manager (future step).
        // For now, just return a value to show the shape.
        Ok(1 + 1)
    }).await
}
```

Here are minimal, end-user oriented examples showing how you would use this library.

- In your Cargo.toml, depend only on the facade crate and select a backend via repository features (no direct backend crates needed):
  - repository = { package = "storeit", path = "./storeit", features = ["libsql-backend"] }
  - Swap the feature for other backends:
    - Postgres: repository = { package = "storeit", path = "./storeit", features = ["postgres-backend"] }
    - MySQL: repository = { package = "storeit", path = "./storeit", features = ["mysql-async"] }

1) Define your entity with #[derive(Entity)] and generate a typed repository with #[repository]

```rust
#![allow(unexpected_cfgs)]
use repository::{Entity, RowAdapter};

#[derive(Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

// The derive also creates a default row adapter named `UserRowAdapter`.
// You can use that (when the backend’s macro features are enabled) or write your own.
// For transactions, use `repository::transactions::default_transaction_template()` to get
// a backend-agnostic TransactionTemplate. Concrete managers are provided by backends
// (e.g., `repository::backends::LibsqlTransactionManager`).
```

2) Pick a backend and generate a typed repository API

The repository attribute macro generates a small module with a `Repository<A>` type and any `find_by_<field>` helpers you request. It delegates to the chosen backend.

Example for libsql (SQLite) with a `find_by_email` helper:

```rust
#![allow(unexpected_cfgs)]
use repository::{repository, RowAdapter};

#[repository(entity = User, backend = Libsql, finders(find_by_email: String))]
pub mod users_repo {}

// The macro above generates:
// - users_repo::Repository<A>
// - impl Repository<User> for that type (async CRUD)
// - an inherent method `find_by_email(&self, value: &String)`
```

3) Use it in your application (libsql example)

```rust
#![allow(unexpected_cfgs)]
use repository::{RowAdapter, Repository};

#[derive(repository::Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

#[repository::repository(entity = User, backend = Libsql, finders(find_by_email: String))]
pub mod users_repo {}

// If you prefer an explicit adapter, you can implement RowAdapter<User> manually:
struct UserAdapter;
impl RowAdapter<User> for UserAdapter {
    type Row = libsql::Row;
    fn from_row(&self, row: &Self::Row) -> repository::RepoResult<User> {
        let id: i64 = row.get(0)?;
        let email: String = row.get(1)?;
        let active_i64: i64 = row.get(2)?; // 0/1 in SQLite
        Ok(User { id: Some(id), email, active: active_i64 != 0 })
    }
}

#[tokio::main]
async fn main() -> repository::RepoResult<()> {
    // Build the typed repository directly from a connection URL.
    // Note: Ensure your schema is created via your usual migration tool before running.
    let users: users_repo::Repository<UserRowAdapter> = users_repo::Repository::from_url(
        "file::memory:?cache=shared",
    ).await?;

    // Insert
    let created = users.insert(&User { id: None, email: "a@example.com".into(), active: true }).await?;

    // Find by id
    let by_id = users.find_by_id(&created.id.unwrap()).await?;
    assert!(by_id.is_some());

    // Finder generated by the macro
    let found = users.find_by_email(&"a@example.com".to_string()).await?;
    assert_eq!(found.len(), 1);

    // Update
    let mut u = found.into_iter().next().unwrap();
    u.active = false;
    let _updated = users.update(&u).await?;

    // Delete
    let ok = users.delete_by_id(&u.id.unwrap()).await?;
    assert!(ok);

    Ok(())
}
```

Notes:
- To run the libsql example, enable the feature on the facade dependency: repository = { package = "storeit", path = "./storeit", features = ["libsql-backend"] }.
- For Postgres or MySQL, swap the feature to "postgres-backend" or "mysql-async" accordingly. No direct backend crate dependencies are required in your Cargo.toml.

4) Using transactions (backend-agnostic) in your app

The transactions API lets you structure business logic around transactional units of work without touching backend crates or types. The canonical, end‑user starting point is to use a single, entity‑agnostic transaction manager for your whole application.

```rust
#![allow(unexpected_cfgs)]
use repository::Repository;
use repository::transactions::{TransactionTemplate, TransactionManager, TransactionContext, default_transaction_template};

#[derive(repository::Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

#[derive(repository::Entity, Clone, Debug, PartialEq)]
pub struct Order {
    #[fetch(id)]
    pub id: Option<i64>,
    pub user_id: i64,
}

#[repository::repository(entity = User, backend = Libsql, finders(find_by_email: String))]
pub mod users_repo {}

#[repository::repository(entity = Order, backend = Libsql)]
pub mod orders_repo {}

#[tokio::main]
async fn main() -> repository::RepoResult<()> {
    // Create repositories once (e.g., as singletons) and reuse across transactions/call sites.
    let users: users_repo::Repository<UserRowAdapter> = users_repo::Repository::from_url(
        "file::memory:?cache=shared",
    ).await?;
    let orders: orders_repo::Repository<OrderRowAdapter> = orders_repo::Repository::from_url(
        "file::memory:?cache=shared",
    ).await?;

    // Grab a single, entity-agnostic transaction template for your app.
    // You can reuse this to transact over multiple entities/repositories.
    let tpl = default_transaction_template();

    // Reuse the repositories inside transactions without recreating them.
    tpl.execute(|_ctx: TransactionContext| async move {
        let created_user = users.insert(&User { id: None, email: "txn@example.com".into(), active: true }).await?;
        // Hypothetical usage across another entity inside same transaction
        let _created_order = orders.insert(&Order { id: None, user_id: created_user.id.unwrap() }).await?;
        Ok(())
    }).await?;

    Ok(())
}
```

Notes on transactions:
- default_transaction_template() gives you a single, entity‑agnostic TransactionTemplate you can pass around your app.
- Concrete managers: libsql already ships a concrete `repository::backends::LibsqlTransactionManager` implementing the trait and actually controlling transactions (BEGIN/COMMIT/ROLLBACK, savepoints, read‑only, busy_timeout best‑effort). Postgres and MySQL managers are also available behind feature flags.
- Pattern: create repositories once (singletons) and reuse them everywhere. Inside `tpl.execute`, repository methods will automatically participate in the active transaction for all backends (MySQL via task‑local pickup; LibSQL and Postgres now also prefer the task‑local tx handle on each call). You can still use the manager’s repository(ctx, adapter) helper if you prefer explicitly tx‑bound repos. Keep your business functions generic over `M: TransactionManager`. 
- Mapping details per backend (isolation, read‑only, timeouts): see docs/transactions.md.

Defaults and simple overrides:
```ignore
#![allow(unexpected_cfgs)]
use repository::transactions::{default_transaction_template, TransactionDefinition, Propagation, Isolation, TransactionContext};

let tpl = default_transaction_template(); // Required, Default isolation, read/write, no timeout

// Per-call override to run read-only with a 1s timeout
let ro = TransactionDefinition { propagation: Propagation::Required, isolation: Isolation::Default, read_only: true, timeout: Some(std::time::Duration::from_secs(1)) };

tpl.execute_with(&ro, |_ctx: TransactionContext| async move {
    // queries only
    Ok::<_, repository::RepoError>(())
}).await?;
```

Backend-specific transaction quickstart:
- libsql: use `LibsqlTransactionManager::repository(ctx, adapter)` to obtain a `LibsqlRepository<_, _>` bound to the current tx connection if active, otherwise a regular one.
- Postgres: use `TokioPostgresTransactionManager::repository(ctx, adapter)` to get a `TokioPostgresRepository<_, _>` bound to the active tx client when present.
- MySQL: use `MysqlAsyncTransactionManager::repository(ctx, adapter)`. The returned `MysqlAsyncRepository<_, _>` automatically picks up the task-local transaction connection for operations within an active transaction.

Libsql manager note (best‑effort mapping) — example skeleton:
```ignore
#![allow(unexpected_cfgs)]
use repository::{backends::LibsqlTransactionManager, transactions::{TransactionTemplate, TransactionDefinition, Propagation, Isolation, TransactionContext}};
use repository::{Repository, RowAdapter};
use std::sync::Arc;

#[derive(repository::Entity, Clone, Debug, PartialEq)]
pub struct User { #[fetch(id)] pub id: Option<i64>, pub email: String, pub active: bool }
#[repository::repository(entity = User, backend = Libsql)]
pub mod users_repo {}

#[tokio::main]
async fn main() -> repository::RepoResult<()> {
    // Build the manager from an Arc<libsql::Database>
    let db = Arc::new(libsql::Database::open("file::memory:?cache=shared")?);
    // Apply schema here...
    let mgr = LibsqlTransactionManager::from_arc(db.clone());
    let tpl = TransactionTemplate::new(mgr.clone());

    // Reuse a pre-built repo outside transactions if you like
    let repo_outside: users_repo::Repository<UserRowAdapter> = users_repo::Repository::from_url("file::memory:?cache=shared").await?;

    // Or, get a tx‑bound repo inside the transaction via the manager helper
    let def = TransactionDefinition{ propagation: Propagation::Required, isolation: Isolation::Default, read_only: false, timeout: None };
    tpl.execute_with(&def, |ctx: TransactionContext| async move {
        let repo_in_tx: storeit_libsql::LibsqlRepository<User, UserRowAdapter> = mgr.repository(ctx, UserRowAdapter).await?;
        let _u = repo_in_tx.insert(&User{ id: None, email: "x@x".into(), active: true }).await?;
        Ok(())
    }).await?;
    let _ = repo_outside; // demonstrate both patterns
    Ok(())
}
```

## Migration notes: anyhow → RepoError/RepoResult
- As of Aug 2025, repository APIs and examples use storeit_core::RepoError and RepoResult<T> instead of anyhow.
- What you need to do when upgrading:
  - Remove anyhow from your dependencies (prod and dev/tests) where it was only used to satisfy repository API types.
  - Change function signatures returning anyhow::Result<T> to storeit::RepoResult<T> (or storeit_core::RepoResult<T> when using core directly).
  - Map driver errors with storeit::RepoError::backend(e) and row-mapping errors with storeit::RepoError::mapping(e).
  - In generic Ok::<_, E>(...) annotations, use storeit::RepoError.

## The next step (small, incremental tasks)
1. Finalize core repository-facing traits in `storeit_core`.  ✓
   - Async `Repository<T>` with common operations; `RowAdapter<T>`; `ParamValue` for backend-agnostic params. ✓
2. Provide `#[derive(Entity)]` in `storeit_macros` to emit metadata and value extraction, plus `RowAdapter` generation. ✓
3. Provide `#[repository(...)]` attribute macro that wires a chosen backend and generates `find_by_*` methods. ✓
4. Add examples and doc tests to demonstrate intended usage. ✓
   - See `repository` crate docs and examples. ✓
5. Implement initial adapters for Postgres, MySQL, and SQLite-family (libsql). ✓

## Stability and SemVer

- We follow a documented stability policy prior to 1.0 with experimental additions gated behind an `unstable` feature across crates.
- MSRV is 1.70. See docs/api_stability.md for details on SemVer, deprecations, and support window.

## Observability (tracing/metrics)

- The optional features `tracing` and `metrics` are available on backend crates (storeit_libsql, storeit_mysql_async, storeit_tokio_postgres).
- Versioning note: the `tracing` crate’s current stable line on crates.io is 0.1.x (there is no 1.x at this time). We depend on a broad semver range ">=0.1, <0.2" so you can use the latest 0.1.x release. Bring your own subscriber (e.g., tracing-subscriber) in your application.
- These features are off by default; enabling them does not change public APIs.

## Connection management & pooling

- Guidance for pool sizing, timeouts, health checks, and retry/backoff patterns is provided in docs/connection_pools.md.
- Summary:
  - Use bounded pools and per-operation timeouts; keep transactions short.
  - Implement cheap health checks (e.g., `SELECT 1`) for readiness.
  - Only retry idempotent operations, with jittered backoff and caps.

## Security

- SQL strings are always paired with parameter placeholders; values are never interpolated into SQL.
- Identifiers for entities (table/columns) are validated by the derive macro. Do not accept arbitrary user-provided column names at runtime.
- See docs/security.md for guidance on safe patterns and robustness.

## Migrations

- See docs/migrations.md for guidance and examples integrating refinery and sqlx::migrate!.
- Recommendation: run migrations once at startup (or in a separate admin job) before constructing repositories; avoid concurrent runners.

## Cross-platform support

- CI validates core crates and the SQL builder across Linux, macOS, and Windows.
- See docs/platform_notes.md for OS-specific tips (local DBs, SSL libs on Windows, Docker notes, etc.).

## Documentation site

- A browsable documentation site is built with mdBook from the content under docs/ and published via GitHub Pages.
- On pushes to main, the workflow “Docs Site (mdBook)” publishes to the gh-pages branch. Visit your repository’s GitHub Pages URL to view it.

## Release automation

- We use cargo-release with configuration in release.toml. Tags follow the pattern `<crate>-v<version>` (per-crate tags) or `v<version>` for repo-wide tags.
- CHANGELOG.md follows “Keep a Changelog” and is updated during releases.
- GitHub Actions workflow “Release (cargo-release)” runs on tag pushes and can publish to crates.io when `CARGO_REGISTRY_TOKEN` is configured; otherwise it performs a dry-run.
- Conventional Commits are encouraged; an advisory PR title check runs in CI but is non-blocking.

## Production-readiness roadmap (proposed)
- [x] Add continuous integration (format, clippy, tests) and set MSRV.  
  - CI workflow added with stable + MSRV (1.70) matrix; rust-version pinned in all crates.
- [x] Expand SQL builder coverage (e.g., selective UPDATE fields, pagination, sorting).  
  - Added select_all, select_by_field, and select_with_pagination helpers with tests.
- [x] Introduce derive/attribute macros to generate `find_by_<field>` methods.
  - The `#[repository(...)]` macro supports `finders(find_by_email: String, ...)` and generates inherent methods like `find_by_email(&self, value: &String) -> storeit_core::RepoResult<Vec<Entity>>` delegating to `find_by_field`.
- [x] Unified error type across the workspace.  
  - Introduced and standardized on storeit_core::RepoError and RepoResult<T> for backend-agnostic error handling. Public APIs and examples no longer use anyhow.
- [x] Provide example apps in an `examples/` directory (e.g., a tiny SQLite demo).  
  - Added repository/examples/basic.rs demonstrating Entity metadata and the facade usage.
- [x] Document feature matrices and backend-specific notes.
  - Added a detailed section “Backend-specific notes” with feature flags, type bounds, limitations, and usage snippets for mysql_async, libsql, and tokio_postgres.
- [ ] Add license and contribution guidelines before publishing to crates.io.

## Design notes
- Keep `storeit_core` free from DB-specific dependencies; use small adapter traits to bridge to backends.
- Prefer macro-generated metadata over runtime reflection to stay zero-cost and compile-friendly.
- Embrace feature flags for backend-specific crates later (`storeit_libsql`, `storeit_mysql_async`, `storeit_tokio_postgres`, etc.).

## Development
- Rust 2021, workspace-managed.
- Build: `cargo build` at the workspace root.
- Test: `cargo test` (unit tests in builders; doc-tests in the facade).

### Running backend integration tests (with and without containers)
The workspace contains real-database integration tests for the backend adapters (Postgres via tokio_postgres, MySQL via mysql_async, and SQLite/libsql). These tests are marked `#[ignore]` by default to keep developer runs fast and deterministic. You can run them either without containers (skip mode) or with containers using `testcontainers`.

- Prerequisites for container runs:
  - Docker installed and available in PATH
  - Network access to pull test images (GitHub runners have it by default)

- Makefile target (recommended):
  - Skip containers (default):
    - `make integration-backends` (internally sets `SKIP_CONTAINER_TESTS=1` by default)
    - Result: the integration tests compile and then detect skip mode; they finish quickly without starting containers.
  - Run with containers:
    - `RUN_CONTAINERS=1 make integration-backends`
    - Equivalent: `SKIP_CONTAINER_TESTS=0 make integration-backends`

- Full coverage including integration tests:
  - Skip containers (fast & deterministic):
    - `make coverage-all` (merges unit/doctest coverage plus integration test binaries in skip mode)
  - With containers (runs real DBs and merges into coverage):
    - `RUN_CONTAINERS=1 make coverage-all`
    - Alternatively: `SKIP_CONTAINER_TESTS=0 make coverage-all`
  - Print concise summary instead of HTML:
    - `make coverage-all-summary`

- Run individual backends directly with cargo (integration tests only):
  - Libsql (in-memory; still marked ignored to keep default runs quick):
    - `cargo llvm-cov --package storeit_libsql --features libsql-backend -- --ignored`
  - Postgres (testcontainers):
    - With containers: `SKIP_CONTAINER_TESTS=0 cargo llvm-cov --package storeit_tokio_postgres --features postgres-backend -- --ignored`
    - Skip containers: `SKIP_CONTAINER_TESTS=1 cargo llvm-cov --package storeit_tokio_postgres --features postgres-backend -- --ignored`
  - MySQL (testcontainers):
    - With containers: `SKIP_CONTAINER_TESTS=0 cargo llvm-cov --package storeit_mysql_async --features mysql-async -- --ignored`
    - Skip containers: `SKIP_CONTAINER_TESTS=1 cargo llvm-cov --package storeit_mysql_async --features mysql-async -- --ignored`

- Notes and safeguards:
  - Tests perform a quick `docker version` check and skip gracefully if Docker is unavailable, even when `SKIP_CONTAINER_TESTS=0`.
  - Postgres/MySQL tests include connection/migration retries to accommodate container startup time.
  - In CI, a dedicated workflow `.github/workflows/integration-backends.yml` runs these tests with containers enabled.

## Test coverage
There are two straightforward ways to get coverage for this workspace:

- Quick start (aliases):
  - First-time setup (once per machine):
    - rustup component add llvm-tools-preview
    - cargo install cargo-llvm-cov
  - Then use these cargo aliases from the repo root:
    - cargo coverage            # run coverage for whole workspace with all features
    - cargo coverage-html       # generate HTML report at target/llvm-cov/html/index.html
    - cargo coverage-lcov       # generate lcov.info in repo root
  - Open the HTML report:
    - macOS: open target/llvm-cov/html/index.html
    - Linux: xdg-open target/llvm-cov/html/index.html

- Local (full commands without aliases): cargo-llvm-cov
  - Install once:
    - rustup component add llvm-tools-preview
    - cargo install cargo-llvm-cov
  - Run for the whole workspace with all features:
    - cargo llvm-cov --workspace --all-features
  - Generate and open an HTML report:
    - cargo llvm-cov --workspace --all-features --html && open target/llvm-cov/html/index.html
      - On Linux use: xdg-open target/llvm-cov/html/index.html
  - Export LCOV for external services:
    - cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info

- CI (GitHub Actions): a workflow at .github/workflows/coverage.yml runs coverage on push/PR, uploads HTML and LCOV artifacts, and can optionally upload to Codecov.
  - Enable Codecov (optional):
    - Public repo: tokenless uploads usually work. Uncomment the Codecov step in .github/workflows/coverage.yml.
    - Private repo: add CODECOV_TOKEN in repo secrets and uncomment the Codecov step.
  - Add a badge (optional):
    - After enabling Codecov, create a badge at codecov.io for this repository and paste it here.


## Backend-specific notes

See docs/architecture.md for the current architecture and backend overview.

- SQL builder (storeit_sql_builder)
  - Placeholder styles are controlled via crate features on storeit_sql_builder:
    - tokio_postgres -> $1, $2, ...
    - others -> ?
  - Optional: enable feature `libsql_returning` (in storeit_sql_builder and storeit_libsql) to append `RETURNING <id>` for libsql inserts.
  - Builders only generate SQL strings; they do not execute queries.
  - Null semantics: SELECT ... WHERE field = NULL yields no rows in SQL. Use the helpers `select_by_is_null::<E>("field")` or `select_by_is_not_null::<E>("field")` as needed.
  - Additional helpers: `select_by_in::<E>(field, count)`, `select_by_not_in::<E>(field, count)` emit IN/NOT IN with correct placeholder styles.

- libsql adapter (storeit_libsql with feature libsql-backend)
  - Enable in your Cargo.toml:
    - storeit_libsql = { path = "./storeit_libsql", features = ["libsql-backend"] }
    - Optional: to use INSERT ... RETURNING, add feature `libsql_returning` in both storeit_libsql and storeit_sql_builder.
  - Type: storeit_libsql::LibsqlRepository<T, A>
  - Requires T: storeit_core::Fetchable + Identifiable + Insertable + Updatable
  - Requires A: storeit_core::RowAdapter<T, Row = libsql::Row>
  - Supported methods: find_by_id, find_by_field, insert, update, delete_by_id.
  - Example:
    ```ignore
    use storeit_libsql::LibsqlRepository;
    use repository::{Entity, RowAdapter};
    use libsql::Row;

    #[derive(Entity, Clone)]
    #[entity(table = "users")]
    struct User {
        #[fetch(id)]
        id: Option<i64>,
        email: String,
    }

    // Using the auto-generated `UserRowAdapter` would require enabling appropriate feature flags.
    // For explicitness, here's a manual RowAdapter example signature:
    struct UserAdapter;
    impl RowAdapter<User> for UserAdapter {
        type Row = Row;
        fn from_row(&self, row: &Self::Row) -> repository::RepoResult<User> {
            let id: i64 = row.get(0)?;
            let email: String = row.get(1)?;
            Ok(User { id: Some(id), email })
        }
    }

    # async fn demo() -> repository::RepoResult<()> {
    let repo = LibsqlRepository::from_url("file:./db.sqlite3", UserAdapter).await?;
    let _ = repo.find_by_id(&1).await?;
    # Ok(()) }
    ```

- mysql_async adapter (storeit_mysql_async with feature mysql-async)
  - Enable in your Cargo.toml:
    - storeit_mysql_async = { path = "./storeit_mysql_async", features = ["mysql-async"] }
  - Type: storeit_mysql_async::MysqlAsyncRepository<T, A>
  - Requires T: storeit_core::Fetchable + Identifiable + Insertable + Updatable
  - Requires A: storeit_core::RowAdapter<T, Row = mysql_async::Row>
  - Supported methods: find_by_id, find_by_field, insert, update, delete_by_id.
  - Example:
    ```ignore
    use repository::{Entity, RowAdapter};
    use storeit_mysql_async::MysqlAsyncRepository;
    use mysql_async::Row;

    #[derive(Entity, Clone)]
    #[entity(table = "users")]
    struct User { #[fetch(id)] id: Option<i64>, email: String }

    struct UserAdapter;
    impl RowAdapter<User> for UserAdapter {
        type Row = Row;
        fn from_row(&self, row: &Self::Row) -> repository::RepoResult<User> {
            let id: i64 = row.get("id").ok_or_else(|| repository::RepoError::mapping(std::io::Error::new(std::io::ErrorKind::Other, "missing id")))?;
            let email: String = row.get("email").ok_or_else(|| repository::RepoError::mapping(std::io::Error::new(std::io::ErrorKind::Other, "missing email")))?;
            Ok(User { id: Some(id), email })
        }
    }

    # async fn demo() -> repository::RepoResult<()> {
    let repo = MysqlAsyncRepository::from_url(
        "mysql://user:pass@localhost:3306/db",
        UserAdapter,
    ).await?;
    let _ = repo.find_by_id(&1).await?;
    # Ok(()) }
    ```

- tokio_postgres adapter (storeit_tokio_postgres with feature postgres-backend)
  - Enable in your Cargo.toml:
    - storeit_tokio_postgres = { path = "./storeit_tokio_postgres", features = ["postgres-backend"] }
  - Type: storeit_tokio_postgres::TokioPostgresRepository<T, A>
  - Requires T: storeit_core::Fetchable + Identifiable + Insertable + Updatable
  - Requires A: storeit_core::RowAdapter<T, Row = tokio_postgres::Row>
  - Supported methods: find_by_id, find_by_field, insert, update, delete_by_id.

- Limitations and notes
  - The facade crate re-exports core traits and macros; optionally re-export sql builders behind feature "sql-builder".



## Crate layout and merging

We intentionally split the workspace into multiple crates (core, macros, builder, per‑backend adapters, and a facade) to keep dependencies optional, compile times reasonable, and versioning flexible. 