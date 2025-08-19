# Database migrations: integrating refinery and sqlx

Last updated: 2025-08-19

This guide shows minimal, practical ways to run schema migrations before you construct and use repositories. The focus is on examples using refinery and sqlx’s migration tooling. We also include a tiny MigrationRunner trait sketch to keep your application code backend-agnostic.

Notes:
- These snippets are examples; adjust for your project layout and error handling.
- Prefer running migrations on process startup (once) or via a separate admin job. Avoid racing multiple concurrent migration runners.

## When to run migrations

- On service startup, before constructing repositories and serving traffic.
- In CI/CD pipelines or an admin tool (e.g., one-off job), if you want stricter separation of duties.
- Ensure only one instance runs migrations at a time to avoid conflicts.

## refinery

refinery allows you to embed SQL files at compile time and apply them at runtime across multiple backends (Postgres/MySQL/SQLite). It supports common async drivers by providing synchronous apply APIs that you can call inside a blocking section, or you can use driver-specific integration crates.

Directory layout (example):
```
my-app/
  migrations/
    V1__create_users.sql
    V2__add_index.sql
```

Minimal Postgres example using tokio_postgres:
```ignore
use tokio_postgres::{NoTls};
use refinery::embed_migrations;

// Point the macro to your migrations dir (relative to the crate root)
embed_migrations!("./migrations");

pub async fn run_pg_migrations(conn_str: &str) -> storeit_core::RepoResult<()> {
    let (client, connection) = tokio_postgres::connect(conn_str, NoTls).await?;
    tokio::spawn(async move { let _ = connection.await; });

    // refinery works with std::borrow::Borrow<dyn postgres client>; use the provided adapter
    refinery::tokio_postgres::migrate(&client, migrations::runner()).await?;
    Ok(())
}
```

MySQL (mysql_async) example:
```ignore
use mysql_async::{prelude::*, Pool};
use refinery::embed_migrations;
embed_migrations!("./migrations");

pub async fn run_mysql_migrations(url: &str) -> storeit_core::RepoResult<()> {
    let pool = Pool::new(url);
    let mut conn = pool.get_conn().await?;
    // Use refinery MySQL runner; it executes each SQL script in order
    refinery::mysql_async::migrate(&mut conn, migrations::runner()).await?;
    Ok(())
}
```

SQLite/libSQL example (file-backed):
```ignore
use libsql::Database;
use refinery::embed_migrations;
embed_migrations!("./migrations");

pub async fn run_sqlite_migrations(path: &str) -> storeit_core::RepoResult<()> {
    // e.g., path = "file:./db.sqlite3?mode=rwc" or just "./db.sqlite3"
    #[allow(deprecated)]
    let db = Database::open(path)?;
    let conn = db.connect()?;
    refinery::libsql::migrate(&conn, migrations::runner()).await?;
    Ok(())
}
```

Tips:
- Keep migrations idempotent at the SQL level where possible (IF NOT EXISTS) to simplify local dev.
- For Postgres, you can protect migrations with an advisory lock to avoid concurrent runners.

## sqlx::migrate!

sqlx ships a simple migration system that scans a folder of numbered migrations and applies them.

Directory layout required by sqlx (example):
```
my-app/
  migrations/
    20240101_120000_create_users.sql
    20240102_090000_add_index.sql
```

Postgres example:
```ignore
use sqlx::postgres::PgPoolOptions;

pub async fn run_sqlx_pg(conn_str: &str) -> storeit_core::RepoResult<()> {
    let pool = PgPoolOptions::new().max_connections(5).connect(conn_str).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(())
}
```

MySQL example:
```ignore
use sqlx::mysql::MySqlPoolOptions;

pub async fn run_sqlx_mysql(conn_str: &str) -> storeit_core::RepoResult<()> {
    let pool = MySqlPoolOptions::new().max_connections(5).connect(conn_str).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(())
}
```

SQLite example (file-backed):
```ignore
use sqlx::sqlite::SqlitePoolOptions;

pub async fn run_sqlx_sqlite(path: &str) -> storeit_core::RepoResult<()> {
    let pool = SqlitePoolOptions::new().max_connections(5).connect(path).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(())
}
```

## A tiny MigrationRunner trait (sketch)

You can keep application code backend-agnostic by defining a tiny trait and providing small adapters per backend:

```ignore
pub trait MigrationRunner {
    async fn migrate(&self) -> storeit_core::RepoResult<()>;
}

pub struct PgRefineryRunner { pub url: String }
#[async_trait::async_trait]
impl MigrationRunner for PgRefineryRunner {
    async fn migrate(&self) -> storeit_core::RepoResult<()> {
        run_pg_migrations(&self.url).await
    }
}

pub struct MySqlSqlxRunner { pub url: String }
#[async_trait::async_trait]
impl MigrationRunner for MySqlSqlxRunner {
    async fn migrate(&self) -> storeit_core::RepoResult<()> {
        run_sqlx_mysql(&self.url).await
    }
}
```

Using it during startup (pseudocode):
```ignore
#[tokio::main]
async fn main() -> storeit_core::RepoResult<()> {
    // 1) Pick your runner (env-driven)
    let runner = PgRefineryRunner { url: std::env::var("POSTGRES_URL")? };

    // 2) Ensure only one runner does work (e.g., run in an admin job, or guard with advisory lock)
    runner.migrate().await?;

    // 3) Construct repositories and serve traffic
    // ...
    Ok(())
}
```

## Ordering, transactions, and races

- Ordering: both refinery and sqlx order migrations lexicographically by filename; choose a clear, sortable scheme (e.g., timestamps or V<N> prefixes).
- Transactions:
  - Postgres: DDL is transactional; prefer transactional migrations.
  - MySQL: some DDL statements are implicit-commit; plan accordingly.
  - SQLite: supports transactional DDL but some pragmas and schema changes commit implicitly.
- Races: run migrations in a single leader or one-off job. For Postgres, use `pg_try_advisory_lock` to serialize runners; for MySQL/SQLite, coordinate via orchestration (initContainers, leader election) or an application-level lock.

## Minimal approach for this workspace

For the examples under `repository/examples`, we create schemas inline to keep examples self-contained. In real applications, prefer a migrations tool per above to manage schema evolution.
