# Connection management, pooling, and retries

Last updated: 2025-08-19

This document provides practical guidance for sizing pools, setting timeouts, performing health checks, and applying retries across the supported backends. All guidance is non‑binding and aims to help you avoid common pitfalls.

Note: storeit‑rs does not ship its own pool. Each backend uses its driver’s connection primitives. You are free to choose a pooling crate that fits your stack (e.g., mysql_async::Pool, deadpool‑postgres, bb8, etc.).

## Quick heuristics (production‑leaning)

- General sizing: start with pool size ~= CPU cores for write‑heavy, 2–4× cores for read‑heavy workloads; measure and adjust.
- Avoid unbounded pools. Cap acquisition wait time (e.g., 100–1000 ms) and fail fast with a clear error.
- Timeouts: keep network and query timeouts finite. Prefer per‑operation timeouts rather than global process timeouts.
- Health monitoring: expose a cheap “SELECT 1” (or equivalent) and consider periodic background checks.

## LibSQL (SQLite family via libsql)

- Recommended: use a shared in‑memory database only for demos/tests. For applications, use a file or remote libSQL endpoint.
- Busy handling: set PRAGMA busy_timeout to reduce SQLITE_BUSY under contention. The LibsqlTransactionManager in this repo sets a best‑effort busy_timeout using the transaction definition’s timeout.
- Concurrency: SQLite is a single‑writer system; batch writes and keep transactions short. Prefer BEGIN IMMEDIATE for write-heavy workloads.

Health check example (non‑compiled snippet):
```rust
#[allow(unexpected_cfgs)]
async fn libsql_health_check(db: &libsql::Database) -> storeit_core::RepoResult<()> {
    let conn = db.connect()?;
    conn.execute("SELECT 1", ()).await?;
    Ok(())
}
```

## MySQL (mysql_async)

- Use mysql_async::Pool with explicit Opts/OptsBuilder. Configure:
  - min/max size (via PoolOpts), and connection TTL if applicable.
  - connection and read/write timeouts.
- Avoid long‑lived connections with no keep‑alive; enable TCP keepalive if relevant in your environment.

Pool + timeouts example (non‑compiled snippet):
```rust
use mysql_async::{Opts, OptsBuilder, Pool};

fn build_mysql_pool(url: &str) -> storeit_core::RepoResult<Pool> {
    let base = Opts::from_url(url)?;
    let mut builder = OptsBuilder::from_opts(base);
    builder.stmt_cache_size(Some(512));
    // If your environment uses DNS load balancing, consider tcp_keepalive time.
    // Timeouts are set via underlying TCP stack or via query hints; mysql_async itself
    // does not expose per‑op timeouts directly, but you can bound futures with tokio::time::timeout.
    let opts = Opts::from(builder);
    let pool = Pool::new(opts);
    Ok(pool)
}
```

Health check:
```rust
use mysql_async::prelude::Queryable;
async fn mysql_health_check(pool: &mysql_async::Pool) -> storeit_core::RepoResult<()> {
    let mut conn = pool.get_conn().await?;
    conn.query_drop("SELECT 1").await?;
    Ok(())
}
```

## Postgres (tokio_postgres)

- tokio_postgres exposes a Client per connection. For pooling, use a community pool like deadpool‑postgres or bb8‑postgres.
- Suggested defaults:
  - max_size: start with CPU cores, scale with read load.
  - connect_timeout: 1–5s; statement_timeout: 100–1000ms for OLTP paths.

deadpool‑postgres example (non‑compiled snippet):
```rust
use deadpool_postgres::{ManagerConfig, Pool, RecyclingMethod};
use tokio_postgres::NoTls;

async fn build_pg_pool(url: &str) -> storeit_core::RepoResult<Pool> {
    let cfg: tokio_postgres::Config = url.parse()?;
    let mgr_cfg = ManagerConfig { recycling_method: RecyclingMethod::Fast }; // or Verified
    let mgr = deadpool_postgres::Manager::from_config(cfg, NoTls, mgr_cfg);
    let pool = Pool::builder(mgr)
        .max_size(16)
        .build()
        .unwrap();
    Ok(pool)
}
```

Health check:
```rust
async fn pg_health_check(pool: &deadpool_postgres::Pool) -> storeit_core::RepoResult<()> {
    let client = pool.get().await?;
    client.batch_execute("SELECT 1").await?;
    Ok(())
}
```

## Timeouts and cancellation

- Postgres: use `SET LOCAL statement_timeout = '<ms>'` inside transactions (our tokio_postgres TransactionManager already sets it when a timeout is provided). Otherwise, apply per‑future timeouts via `tokio::time::timeout`.
- MySQL: bound futures with `tokio::time::timeout`; also consider `innodb_lock_wait_timeout` for lock waits (our MysqlAsyncTransactionManager sets it best‑effort when a timeout is provided).
- LibSQL: use PRAGMA busy_timeout and keep transactions short.

## Retries and backoff

Guidelines:
- Only retry idempotent reads or well‑understood transient errors (e.g., network hiccups, lock timeouts). Do not blindly retry writes.
- Cap total retry time; use jittered exponential backoff.

A tiny generic retry helper (non‑allocating; non‑compiled snippet):
```rust
use std::time::Duration;
use rand::{thread_rng, Rng};

pub async fn retry<F, Fut, T>(mut op: F, attempts: usize, base_delay: Duration) -> storeit_core::RepoResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = storeit_core::RepoResult<T>>,
{
    let mut last_err: Option<storeit_core::RepoError> = None;
    for i in 0..attempts {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_err = Some(e);
                if i + 1 == attempts { break; }
                // exponential backoff with jitter
                let pow = 1u64 << i.min(10);
                let base = base_delay.as_millis() as u64 * pow;
                let jitter = thread_rng().gen_range(0..(base / 2 + 1));
                let sleep_ms = base + jitter;
                tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| storeit_core::RepoError::backend(std::io::Error::new(std::io::ErrorKind::Other, "retry exhausted"))))
}
```

Usage example (read path):
```rust
// Retrying a finder with ParamValue when the backend may temporarily fail.
use repository::{Repository as _, ParamValue};

async fn safe_find_by_email<R, T>(repo: &R, email: String) -> repository::RepoResult<Vec<T>>
where
    R: repository::Repository<T> + Send + Sync,
    T: repository::Identifiable + Send + Sync + 'static,
{
    retry(|| async { repo.find_by_field("email", ParamValue::String(email.clone())).await }, 3, std::time::Duration::from_millis(50)).await
}
```

## Health endpoints and readiness

- For web services, expose two endpoints:
  - liveness: always OK if process is responsive.
  - readiness: run backend health check(s) with a short timeout; report failure when pool is saturated or DB is unavailable.

## Summary checklist

- [ ] Pool sizes bounded and tuned; connection acquisition bounded.
- [ ] Per‑operation timeouts applied; transaction timeouts configured where supported.
- [ ] Health checks implemented per backend.
- [ ] Retries used only where safe; backoff with jitter and capped attempts.
- [ ] Transactions short‑lived; keep writers fast to avoid lock contention.
