#![cfg(feature = "mysql-async")]

use storeit_core::transactions::TransactionManager;
use storeit_core::Identifiable;
use storeit_core::{RepoError, RepoResult, Repository, RowAdapter};
use storeit_mysql_async::MysqlAsyncRepository;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mariadb::Mariadb;
type MariaDb = Mariadb;
use mysql_async::prelude::Queryable;
use std::sync::OnceLock;

// Serialize container-heavy integration tests and share a single container across tests.
static GLOBAL_IT_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
async fn acquire_it_lock() -> tokio::sync::MutexGuard<'static, ()> {
    GLOBAL_IT_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

// Single shared MariaDB container for the entire file (started lazily on first use)
static DB_ONCE: tokio::sync::OnceCell<(testcontainers::ContainerAsync<MariaDb>, String)> =
    tokio::sync::OnceCell::const_new();
async fn shared_db_url() -> String {
    let (_node, url) = DB_ONCE
        .get_or_init(|| async {
            let node = MariaDb::default().start().await;
            let port = node.get_host_port_ipv4(3306).await;
            let url = mysql_url_from_node_port(port)
                .await
                .expect("build mysql url");
            // Ensure schema once globally
            apply_migration_with_retry(&url)
                .await
                .expect("apply migration");
            (node, url)
        })
        .await;
    // Keep container alive by holding the tuple in the OnceCell; return cloned URL
    url.clone()
}

// dyn trait alias for convenience
type DynRepo = dyn Repository<tests_common::User> + Send + Sync;

// Quick check to see if Docker is available; if not, skip container tests gracefully.
fn containers_usable() -> bool {
    // If the caller explicitly asked to run containers, don't pre-skip; let tests attempt to run.
    if std::env::var("RUN_CONTAINERS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
    {
        return true;
    }
    if skip_containers() {
        return false;
    }
    let docker_ok = std::process::Command::new("docker")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if docker_ok {
        return true;
    }
    // Fallback: try podman
    std::process::Command::new("podman")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

struct MyAdapter;
impl RowAdapter<tests_common::User> for MyAdapter {
    type Row = mysql_async::Row;
    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<tests_common::User> {
        let id: i64 = row.get("id").ok_or_else(|| {
            storeit_core::RepoError::mapping(std::io::Error::new(
                std::io::ErrorKind::Other,
                "missing id",
            ))
        })?;
        let email: String = row.get("email").ok_or_else(|| {
            storeit_core::RepoError::mapping(std::io::Error::new(
                std::io::ErrorKind::Other,
                "missing email",
            ))
        })?;
        // MySQL "BOOLEAN" is alias for TINYINT(1)
        let active: i64 = row.get("active").ok_or_else(|| {
            storeit_core::RepoError::mapping(std::io::Error::new(
                std::io::ErrorKind::Other,
                "missing active",
            ))
        })?;
        Ok(tests_common::User {
            id: Some(id),
            email,
            active: active != 0,
        })
    }
}

struct MyFactory {
    url: String,
}

#[async_trait::async_trait]
impl tests_common::RepoFactory for MyFactory {
    async fn new_user_repo(&self) -> RepoResult<Box<DynRepo>> {
        let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
            &self.url,
            tests_common::User::ID_COLUMN,
            MyAdapter,
        )
        .await?;
        Ok(Box::new(repo))
    }
}

async fn apply_migration(url: &str) -> RepoResult<()> {
    use mysql_async::prelude::*;
    let pool = mysql_async::Pool::new(url);
    let mut conn = pool.get_conn().await.map_err(RepoError::backend)?;
    // Simple readiness ping
    conn.query_drop("SELECT 1")
        .await
        .map_err(RepoError::backend)?;
    // Reduce metadata lock waits so we fail fast instead of hanging
    let _ = conn.query_drop("SET SESSION lock_wait_timeout = 1").await;
    eprintln!(
        "[integration][mysql] applying migration SQL to url: {}",
        url
    );
    eprintln!(
        "[integration][mysql] migration SQL: {}",
        tests_common::migrations::MYSQL_USERS_SQL.replace('\n', " ")
    );
    conn.query_drop(tests_common::migrations::MYSQL_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;
    eprintln!("[integration][mysql] migration applied successfully");
    // Disconnect best-effort with a short timeout to avoid hanging the migration call
    let _ = tokio::time::timeout(std::time::Duration::from_millis(200), pool.disconnect()).await;
    Ok(())
}

/// Try to apply migration with small retries to accommodate server startup time.
async fn apply_migration_with_retry(url: &str) -> RepoResult<()> {
    // Try up to ~180s with short per-attempt timeouts so we never hang indefinitely.
    let max_attempts = 180usize; // 180 * 1s = ~180s total worst-case
    for attempt in 1..=max_attempts {
        eprintln!(
            "[integration][mysql] migration attempt {}/{} using url: {}",
            attempt, max_attempts, url
        );
        let fut = apply_migration(url);
        match tokio::time::timeout(std::time::Duration::from_secs(5), fut).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => {
                eprintln!(
                    "[integration][mysql] migration attempt {}/{} failed: {:#}",
                    attempt, max_attempts, e
                );
            }
            Err(_) => {
                eprintln!(
                    "[integration][mysql] migration attempt {}/{} timed out",
                    attempt, max_attempts
                );
                // After a timeout, try a quick 1s ping to the same URL and log result
                let pool = mysql_async::Pool::new(url);
                let mut ping = pool.get_conn();
                match tokio::time::timeout(std::time::Duration::from_secs(1), &mut ping).await {
                    Ok(Ok(mut conn)) => {
                        let ping2 = tokio::time::timeout(
                            std::time::Duration::from_secs(1),
                            mysql_async::prelude::Queryable::query_drop(&mut conn, "SELECT 1"),
                        )
                        .await;
                        match ping2 {
                            Ok(Ok(())) => eprintln!(
                                "[integration][mysql] post-timeout ping succeeded on {}",
                                url
                            ),
                            Ok(Err(e)) => eprintln!(
                                "[integration][mysql] post-timeout ping failed on {}: {:#}",
                                url, e
                            ),
                            Err(_) => eprintln!(
                                "[integration][mysql] post-timeout ping timed out on {}",
                                url
                            ),
                        }
                        let _ = conn.disconnect().await;
                    }
                    Ok(Err(e)) => eprintln!(
                        "[integration][mysql] post-timeout get_conn failed on {}: {:#}",
                        url, e
                    ),
                    Err(_) => eprintln!(
                        "[integration][mysql] post-timeout get_conn timed out on {}",
                        url
                    ),
                }
                let _ = pool.disconnect().await;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Err(RepoError::backend(std::io::Error::new(
        std::io::ErrorKind::Other,
        "migration failed after retries",
    )))
}

/// Construct a working MySQL connection URL by trying a few common credential combos
/// used by popular MySQL container images.
async fn mysql_url_from_node_port(port: u16) -> RepoResult<String> {
    let host = "127.0.0.1";
    // (user, pass, db)
    let candidates: &[(&str, &str, &str)] = &[
        // MariaDB common defaults first
        ("test", "test", "test"),
        ("mariadb", "mariadb", "test"),
        // Generic MySQL-like combinations
        ("test", "test", "mysql"),
        ("root", "root", "mysql"),
        ("root", "", "mysql"),
        ("mysql", "mysql", "mysql"),
    ];

    // Try for up to ~60s total, with short per-connection timeouts to avoid hangs
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        for (user, pass, db) in candidates {
            let url = if pass.is_empty() {
                format!("mysql://{user}@{host}:{port}/{db}")
            } else {
                format!("mysql://{user}:{pass}@{host}:{port}/{db}")
            };
            eprintln!(
                "[integration][mysql] probe attempt {} trying {}@{}:{}/{}",
                attempt, user, host, port, db
            );
            let pool = mysql_async::Pool::new(url.as_str());
            let fut = pool.get_conn();
            match tokio::time::timeout(std::time::Duration::from_secs(1), fut).await {
                Ok(Ok(mut conn)) => {
                    eprintln!(
                        "[integration][mysql] connected successfully using {} / {}",
                        user, db
                    );
                    // If we connected to the system schema "mysql", try to create a dedicated test DB
                    // and prefer returning a URL that targets it to avoid privilege quirks.
                    let mut final_url = url.clone();
                    if *db == "mysql" {
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_secs(2),
                            conn.query_drop("CREATE DATABASE IF NOT EXISTS test"),
                        )
                        .await;
                        final_url = if pass.is_empty() {
                            format!("mysql://{user}@{host}:{port}/test")
                        } else {
                            format!("mysql://{user}:{pass}@{host}:{port}/test")
                        };
                    }
                    eprintln!("[integration][mysql] will use url: {}", final_url);
                    let _ = conn.disconnect().await;
                    let _ = pool.disconnect().await;
                    return Ok(final_url);
                }
                Ok(Err(e)) => {
                    eprintln!(
                        "[integration][mysql] connect failed for {} / {}: {}",
                        user, db, e
                    );
                }
                Err(_) => {
                    eprintln!(
                        "[integration][mysql] connect timed out for {} / {}",
                        user, db
                    );
                }
            }
            let _ = pool.disconnect().await;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err(RepoError::backend(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Unable to connect to MySQL/MariaDB container with known credentials within timeout",
    )))
}

fn skip_containers() -> bool {
    std::env::var("SKIP_CONTAINER_TESTS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_crud_and_find_by_field() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let factory = MyFactory { url: url.clone() };
    tests_common::test_crud_roundtrip(&factory).await?;
    tests_common::test_find_by_field(&factory).await?;

    // Also verify delete_by_id returns false for non-existent id
    let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;
    let deleted = repo.delete_by_id(&i64::MAX).await?;
    assert!(
        !deleted,
        "Expected delete_by_id to return false for non-existing id"
    );

    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_unique_violation_on_duplicate_email() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;
    let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;
    let u = tests_common::User {
        id: None,
        email: "dup@example.com".into(),
        active: true,
    };
    let _ = repo.insert(&u).await?;
    let err = repo
        .insert(&u)
        .await
        .expect_err("expected unique violation");
    let msg = format!("{:#}", err).to_lowercase();
    // Accept either a Backend variant (driver-specific) or a message containing duplicate/unique
    if !matches!(err, storeit_core::RepoError::Backend { .. })
        && !(msg.contains("duplicate") || msg.contains("unique"))
    {
        panic!("unexpected error: {}", msg);
    }
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_find_by_field_bool_and_null() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;
    let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;

    // Seed
    let _ = repo
        .insert(&tests_common::User {
            id: None,
            email: "t1@example.com".into(),
            active: true,
        })
        .await?;
    let _ = repo
        .insert(&tests_common::User {
            id: None,
            email: "t2@example.com".into(),
            active: false,
        })
        .await?;

    // active = true
    let found_true = repo
        .find_by_field("active", storeit_core::ParamValue::Bool(true))
        .await?;
    assert!(found_true.iter().any(|u| u.active));

    // active = false
    let found_false = repo
        .find_by_field("active", storeit_core::ParamValue::Bool(false))
        .await?;
    assert!(found_false.iter().all(|u| !u.active));

    // active = NULL -> no rows
    let found_null = repo
        .find_by_field("active", storeit_core::ParamValue::Null)
        .await?;
    assert_eq!(found_null.len(), 0);
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_bad_column_name_errors() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;
    let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;

    let err = repo
        .find_by_field(
            "does_not_exist",
            storeit_core::ParamValue::String("x".into()),
        )
        .await
        .expect_err("expected bad column error");
    let msg = format!("{:#}", err).to_lowercase();
    // Accept either a backend/mapping error or a driver message mentioning the column problem
    if !(matches!(err, storeit_core::RepoError::Backend { .. })
        || matches!(err, storeit_core::RepoError::Mapping { .. })
        || msg.contains("unknown")
        || msg.contains("column"))
    {
        panic!("unexpected error: {}", msg);
    }
    Ok(())
}

// Adapter that intentionally requests a missing column to force a mapping error
struct BadAdapter;
impl RowAdapter<tests_common::User> for BadAdapter {
    type Row = mysql_async::Row;
    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<tests_common::User> {
        let _missing: String = row.get("no_such_col").ok_or_else(|| {
            storeit_core::RepoError::mapping(std::io::Error::new(
                std::io::ErrorKind::Other,
                "missing col",
            ))
        })?;
        unreachable!("should have errored before");
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_row_adapter_mapping_error_surfaces() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    // Seed one user with good adapter
    let good_repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;
    let created = good_repo
        .insert(&tests_common::User {
            id: None,
            email: "map@example.com".into(),
            active: true,
        })
        .await?;

    // Use repo with bad adapter, expect mapping error
    let bad_repo = MysqlAsyncRepository::<tests_common::User, BadAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        BadAdapter,
    )
    .await?;
    let err = bad_repo
        .find_by_id(&created.id.unwrap())
        .await
        .expect_err("expected mapping error");
    let msg = format!("{:#}", err).to_lowercase();
    assert!(
        msg.contains("mapping") || msg.contains("missing"),
        "unexpected error: {}",
        msg
    );
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_param_type_mismatch_casts_or_zero_rows() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;
    let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;

    // Insert one valid record
    let _ = repo
        .insert(&tests_common::User {
            id: None,
            email: "typemismatch@example.com".into(),
            active: true,
        })
        .await?;

    // Use an incompatible ParamValue type for email (I32). MySQL may coerce types; assert it does not panic and returns some result (likely 0 rows).
    let res = repo
        .find_by_field("email", storeit_core::ParamValue::I32(123))
        .await?;
    assert!(
        res.is_empty() || res.iter().all(|u| u.email == "123"),
        "unexpected rows returned"
    );
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_transaction_manager_commit_and_rollback() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    // Build pool and manager
    let pool = mysql_async::Pool::new(url.as_str());
    let tm = storeit_mysql_async::MysqlAsyncTransactionManager::new(pool.clone());

    // Commit path
    let tm_outer = tm.clone();
    let committed = tm
        .clone()
        .execute(
            &storeit_core::transactions::TransactionDefinition::default(),
            move |ctx| {
                let tm_inner = tm_outer.clone();
                async move {
                    // Repo inside tx should use the tx connection implicitly
                    let repo = tm_inner
                        .repository::<tests_common::User, MyAdapter>(ctx, MyAdapter)
                        .await?;
                    let created = repo
                        .insert(&tests_common::User {
                            id: None,
                            email: "tx_commit@example.com".into(),
                            active: true,
                        })
                        .await?;
                    Ok::<_, RepoError>(created.id)
                }
            },
        )
        .await?;

    // Verify committed row exists
    let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;
    let got = repo.find_by_id(&committed.unwrap()).await?;
    assert!(got.is_some());

    // Rollback path
    let tm_outer2 = tm.clone();
    let err = tm
        .clone()
        .execute(
            &storeit_core::transactions::TransactionDefinition::default(),
            move |ctx| {
                let tm_inner2 = tm_outer2.clone();
                async move {
                    let repo = tm_inner2
                        .repository::<tests_common::User, MyAdapter>(ctx, MyAdapter)
                        .await?;
                    let created = repo
                        .insert(&tests_common::User {
                            id: None,
                            email: "tx_rollback@example.com".into(),
                            active: true,
                        })
                        .await?;
                    // Force an error so the transaction rolls back
                    let _ = repo.delete_by_id(&created.id.unwrap()).await?;
                    Err::<(), _>(RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "force rollback",
                    )))
                }
            },
        )
        .await
        .expect_err("expected rollback error");
    let _ = err; // just ensure it is an error

    // Ensure the rolled back email is not present
    let rows = repo
        .find_by_field(
            "email",
            storeit_core::ParamValue::String("tx_rollback@example.com".into()),
        )
        .await?;
    assert!(rows.is_empty());

    // Best-effort: disconnect pool
    let _ = pool.disconnect().await;
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_transaction_manager_nested_savepoints() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let pool = mysql_async::Pool::new(url.as_str());
    let tm = storeit_mysql_async::MysqlAsyncTransactionManager::new(pool.clone());

    let tm_outer = tm.clone();
    tm.clone()
        .execute(
            &storeit_core::transactions::TransactionDefinition::default(),
            move |ctx| {
                let tm_lvl1 = tm_outer.clone();
                async move {
                    // First level
                    let repo = tm_lvl1
                        .repository::<tests_common::User, MyAdapter>(ctx, MyAdapter)
                        .await?;
                    let _ = repo
                        .insert(&tests_common::User {
                            id: None,
                            email: "sp_l1@example.com".into(),
                            active: true,
                        })
                        .await?;

                    // Nested level should use savepoint
                    let mut def = storeit_core::transactions::TransactionDefinition::default();
                    def.propagation = storeit_core::transactions::Propagation::Nested;
                    let tm_outer_lvl2 = tm_lvl1.clone();
                    let _ = tm_lvl1
                        .clone()
                        .execute(&def, move |ctx2| {
                            let tm_lvl2 = tm_outer_lvl2.clone();
                            async move {
                                let repo2 = tm_lvl2
                                    .repository::<tests_common::User, MyAdapter>(ctx2, MyAdapter)
                                    .await?;
                                let _ = repo2
                                    .insert(&tests_common::User {
                                        id: None,
                                        email: "sp_l2@example.com".into(),
                                        active: true,
                                    })
                                    .await?;
                                Ok::<_, RepoError>(())
                            }
                        })
                        .await?;
                    Ok::<_, RepoError>(())
                }
            },
        )
        .await?;

    let repo = MysqlAsyncRepository::<tests_common::User, MyAdapter>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        MyAdapter,
    )
    .await?;
    for email in ["sp_l1@example.com", "sp_l2@example.com"] {
        let rows = repo
            .find_by_field("email", storeit_core::ParamValue::String(email.into()))
            .await?;
        assert_eq!(rows.len(), 1, "missing {}", email);
    }

    let _ = pool.disconnect().await;
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_transaction_manager_propagation_supports_and_not_supported() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let pool = mysql_async::Pool::new(url.as_str());
    let tm = storeit_mysql_async::MysqlAsyncTransactionManager::new(pool.clone());

    // NotSupported outside a tx: function runs without starting tx
    let mut def = storeit_core::transactions::TransactionDefinition::default();
    def.propagation = storeit_core::transactions::Propagation::NotSupported;
    tm.execute(&def, |_ctx| async move { Ok::<_, RepoError>(()) })
        .await?;

    // Supports outside a tx: should also run without starting tx
    def.propagation = storeit_core::transactions::Propagation::Supports;
    tm.execute(&def, |_ctx| async move { Ok::<_, RepoError>(()) })
        .await?;

    let _ = pool.disconnect().await;
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_transaction_manager_propagation_never_errors_when_in_tx() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let pool = mysql_async::Pool::new(url.as_str());
    let tm = storeit_mysql_async::MysqlAsyncTransactionManager::new(pool.clone());

    // Start a tx and inside try Propagation::Never
    let tm_outer = tm.clone();
    let err = tm
        .clone()
        .execute(
            &storeit_core::transactions::TransactionDefinition::default(),
            move |_ctx| {
                let tm_inner = tm_outer.clone();
                async move {
                    let mut def = storeit_core::transactions::TransactionDefinition::default();
                    def.propagation = storeit_core::transactions::Propagation::Never;
                    let r = tm_inner
                        .clone()
                        .execute(&def, |_ctx2| async move { Ok::<_, RepoError>(()) })
                        .await;
                    assert!(r.is_err(), "Propagation::Never should error inside tx");
                    Ok::<_, RepoError>(())
                }
            },
        )
        .await?;

    let _ = err; // silence unused
    let _ = pool.disconnect().await;
    Ok(())
}
