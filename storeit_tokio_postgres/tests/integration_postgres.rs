#![cfg(feature = "postgres-backend")]
#![allow(unexpected_cfgs)]

use storeit_core::{Identifiable, RepoError, RepoResult, Repository, RowAdapter};
use storeit_tokio_postgres::TokioPostgresRepository;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

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
    std::process::Command::new("podman")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn pg_connect_with_retry(url: &str) -> RepoResult<tokio_postgres::Client> {
    for _ in 0..30usize {
        match tokio_postgres::connect(url, tokio_postgres::NoTls).await {
            Ok((client, connection)) => {
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                return Ok(client);
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
    }
    Err(RepoError::backend(std::io::Error::new(
        std::io::ErrorKind::Other,
        "failed to connect to postgres after retries",
    )))
}

struct A; // test row adapter
impl RowAdapter<tests_common::User> for A {
    type Row = tokio_postgres::Row;
    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<tests_common::User> {
        use tokio_postgres::Row as PgRow;
        let id: i64 = PgRow::try_get(row, "id").map_err(storeit_core::RepoError::mapping)?;
        let email: String =
            PgRow::try_get(row, "email").map_err(storeit_core::RepoError::mapping)?;
        let active: bool =
            PgRow::try_get(row, "active").map_err(storeit_core::RepoError::mapping)?;
        Ok(tests_common::User {
            id: Some(id),
            email,
            active,
        })
    }
}

struct PgFactory {
    url: String,
}

#[async_trait::async_trait]
impl tests_common::RepoFactory for PgFactory {
    async fn new_user_repo(&self) -> RepoResult<Box<DynRepo>> {
        let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
            &self.url,
            tests_common::User::ID_COLUMN,
            A,
        )
        .await?;
        Ok(Box::new(repo))
    }
}

fn skip_containers() -> bool {
    std::env::var("SKIP_CONTAINER_TESTS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

// Serialize container-heavy integration tests and share a single container across tests.
static GLOBAL_IT_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
async fn acquire_it_lock() -> tokio::sync::MutexGuard<'static, ()> {
    GLOBAL_IT_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

// Single shared Postgres container for the entire file (started lazily on first use)
static DB_ONCE: tokio::sync::OnceCell<(testcontainers::ContainerAsync<Postgres>, String)> =
    tokio::sync::OnceCell::const_new();
async fn shared_db_url() -> String {
    let (_node, url) = DB_ONCE
        .get_or_init(|| async {
            let node = Postgres::default().start().await;
            let port = node.get_host_port_ipv4(5432).await;
            let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
            // Ensure schema once globally
            apply_migration_with_retry(&url)
                .await
                .expect("apply migration");
            (node, url)
        })
        .await;
    url.clone()
}

async fn apply_migration(url: &str) -> RepoResult<()> {
    // Connect and ensure server is ready
    let client = pg_connect_with_retry(url).await?;
    client
        .batch_execute("SELECT 1;")
        .await
        .map_err(RepoError::backend)?;
    // Apply schema
    client
        .batch_execute(tests_common::migrations::POSTGRES_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;
    Ok(())
}

async fn apply_migration_with_retry(url: &str) -> RepoResult<()> {
    let max_attempts = 180usize;
    for attempt in 1..=max_attempts {
        eprintln!(
            "[integration][postgres] migration attempt {}/{} using url: {}",
            attempt, max_attempts, url
        );
        match tokio::time::timeout(std::time::Duration::from_secs(5), apply_migration(url)).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => {
                eprintln!(
                    "[integration][postgres] migration attempt {}/{} failed: {:#}",
                    attempt, max_attempts, e
                );
            }
            Err(_) => {
                eprintln!(
                    "[integration][postgres] migration attempt {}/{} timed out",
                    attempt, max_attempts
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    Err(RepoError::backend(std::io::Error::new(
        std::io::ErrorKind::Other,
        "migration failed after retries",
    )))
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_crud_and_find_by_field() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }

    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let factory = PgFactory { url: url.clone() };
    tests_common::test_crud_roundtrip(&factory).await?;
    tests_common::test_find_by_field(&factory).await?;

    // Also verify delete_by_id returns false for non-existent id
    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
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
async fn postgres_unique_violation_on_duplicate_email() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
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
    if !matches!(err, storeit_core::RepoError::Backend { .. })
        && !(msg.contains("unique") || msg.contains("duplicate"))
    {
        panic!("unexpected error: {}", msg);
    }
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_find_by_field_bool_and_null() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;
    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;

    // Seed data
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

    // active = NULL -> no rows (WHERE active = NULL yields unknown)
    let found_null = repo
        .find_by_field("active", storeit_core::ParamValue::Null)
        .await?;
    assert_eq!(found_null.len(), 0);
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_bad_column_name_errors() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;
    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
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
    type Row = tokio_postgres::Row;
    fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<tests_common::User> {
        // try to get a non-existent column name to trigger an error
        let _: String = row
            .try_get("no_such_col")
            .map_err(storeit_core::RepoError::mapping)?;
        unreachable!("should have failed before");
    }
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_row_adapter_mapping_error_surfaces() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    // Seed one user
    let good_repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;
    let created = good_repo
        .insert(&tests_common::User {
            id: None,
            email: "map@example.com".into(),
            active: true,
        })
        .await?;

    // Now use a repo with a bad adapter that will fail on mapping
    let bad_repo = TokioPostgresRepository::<tests_common::User, BadAdapter>::from_url(
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
        msg.contains("mapping") || msg.contains("column") || msg.contains("no such"),
        "unexpected error: {}",
        msg
    );
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_param_type_mismatch_errors() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;
    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
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

    // Use an incompatible ParamValue type for email (I32) to trigger a Postgres type error
    let err = repo
        .find_by_field("email", storeit_core::ParamValue::I32(123))
        .await
        .expect_err("expected type error");
    let msg = format!("{:#}", err).to_lowercase();
    if !matches!(err, storeit_core::RepoError::Backend { .. })
        && !(msg.contains("type")
            || msg.contains("cast")
            || msg.contains("operator")
            || msg.contains("mismatch"))
    {
        panic!("unexpected error: {}", msg);
    }
    Ok(())
}

// --- New transaction-focused tests ---
#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_transaction_commit_persists_using_manager() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    use storeit_core::transactions::{
        Isolation, Propagation, TransactionDefinition, TransactionManager,
    };
    use storeit_tokio_postgres::TokioPostgresTransactionManager;

    let mgr = TokioPostgresTransactionManager::new(url.clone());
    let def = TransactionDefinition {
        propagation: Propagation::Required,
        isolation: Isolation::Default,
        read_only: false,
        timeout: None,
    };
    let email = "tx_commit@pg.example".to_string();

    let res = mgr
        .execute(&def, |_ctx| {
            let url = url.clone();
            let email = email.clone();
            async move {
                let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
                    &url,
                    tests_common::User::ID_COLUMN,
                    A,
                )
                .await?;
                let _ = repo
                    .insert(&tests_common::User {
                        id: None,
                        email: email.clone(),
                        active: true,
                    })
                    .await?;
                Ok::<_, RepoError>(())
            }
        })
        .await;
    assert!(res.is_ok(), "transaction should commit: {:?}", res);

    // After commit, the row should be visible via a fresh repo
    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;
    let found = repo
        .find_by_field("email", storeit_core::ParamValue::String(email))
        .await?;
    assert_eq!(found.len(), 1);
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_read_only_tx_prevents_writes() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    use storeit_core::transactions::{
        Isolation, Propagation, TransactionDefinition, TransactionManager,
    };
    use storeit_tokio_postgres::TokioPostgresTransactionManager;
    let mgr = TokioPostgresTransactionManager::new(url.clone());

    // Read-only tx should fail on write
    let def_ro = TransactionDefinition {
        propagation: Propagation::Required,
        isolation: Isolation::Default,
        read_only: true,
        timeout: None,
    };
    let res = mgr
        .execute(&def_ro, |_ctx| {
            let url = url.clone();
            async move {
                let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
                    &url,
                    tests_common::User::ID_COLUMN,
                    A,
                )
                .await?;
                let err = repo
                    .insert(&tests_common::User {
                        id: None,
                        email: "ro@example.com".into(),
                        active: true,
                    })
                    .await
                    .expect_err("write should fail in read-only tx");
                drop(err); // ensure surfaced
                Ok::<_, RepoError>(())
            }
        })
        .await;
    assert!(
        res.is_ok(),
        "outer execute should succeed while inner write fails"
    );

    // Outside tx, write should succeed
    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;
    let created = repo
        .insert(&tests_common::User {
            id: None,
            email: "rw@example.com".into(),
            active: true,
        })
        .await?;
    assert!(repo.find_by_id(&created.id.unwrap()).await?.is_some());
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_nested_savepoints_commit_and_rollback() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    use storeit_core::transactions::{
        Isolation, Propagation, TransactionDefinition, TransactionManager,
    };
    use storeit_tokio_postgres::TokioPostgresTransactionManager;

    let mgr = TokioPostgresTransactionManager::new(url.clone());
    let outer = TransactionDefinition {
        propagation: Propagation::Required,
        isolation: Isolation::Default,
        read_only: false,
        timeout: None,
    };

    let email_ok = "inner_ok@pg".to_string();
    let email_fail = "inner_fail@pg".to_string();

    let res = mgr
        .execute(&outer, |_ctx_outer| {
            let mgr = mgr.clone();
            let url = url.clone();
            let email_ok = email_ok.clone();
            let email_fail = email_fail.clone();
            async move {
                // Inner successful nested tx
                let inner_ok = TransactionDefinition {
                    propagation: Propagation::Nested,
                    isolation: Isolation::Default,
                    read_only: false,
                    timeout: None,
                };
                let _ = mgr
                    .execute(&inner_ok, |_ctx| {
                        let url = url.clone();
                        let email_ok = email_ok.clone();
                        async move {
                            let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
                                &url,
                                tests_common::User::ID_COLUMN,
                                A,
                            )
                            .await?;
                            let _ = repo
                                .insert(&tests_common::User {
                                    id: None,
                                    email: email_ok,
                                    active: true,
                                })
                                .await?;
                            Ok::<_, RepoError>(())
                        }
                    })
                    .await?;

                // Inner failing nested tx -> rollback
                let inner_fail = TransactionDefinition {
                    propagation: Propagation::Nested,
                    isolation: Isolation::Default,
                    read_only: false,
                    timeout: None,
                };
                let _ = mgr
                    .execute::<(), _, _>(&inner_fail, |_ctx| {
                        let url = url.clone();
                        let email_fail = email_fail.clone();
                        async move {
                            let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
                                &url,
                                tests_common::User::ID_COLUMN,
                                A,
                            )
                            .await?;
                            let _ = repo
                                .insert(&tests_common::User {
                                    id: None,
                                    email: email_fail,
                                    active: false,
                                })
                                .await?;
                            Err::<(), RepoError>(RepoError::backend(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "boom",
                            )))
                        }
                    })
                    .await
                    .expect_err("inner should rollback");

                Ok::<_, RepoError>(())
            }
        })
        .await;
    assert!(res.is_ok(), "outer should commit");

    // Verify only inner_ok row exists
    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;
    let ok = repo
        .find_by_field(
            "email",
            storeit_core::ParamValue::String("inner_ok@pg".into()),
        )
        .await?;
    let fail = repo
        .find_by_field(
            "email",
            storeit_core::ParamValue::String("inner_fail@pg".into()),
        )
        .await?;
    assert_eq!(ok.len(), 1);
    assert_eq!(fail.len(), 0);
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_transaction_repository_reuse_commits() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    // Prebuild a repository OUTSIDE any transaction and reuse it inside.
    let repo_outside = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;

    use storeit_core::transactions::{
        Isolation, Propagation, TransactionDefinition, TransactionManager,
    };
    use storeit_tokio_postgres::TokioPostgresTransactionManager;
    let mgr = TokioPostgresTransactionManager::new(url.clone());
    let def = TransactionDefinition {
        propagation: Propagation::Required,
        isolation: Isolation::Default,
        read_only: false,
        timeout: None,
    };
    let email = format!(
        "reuse_{}@pg",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let res = mgr
        .execute(&def, |_ctx| {
            let repo = repo_outside; // move into async block
            let email = email.clone();
            async move {
                let created = repo
                    .insert(&tests_common::User {
                        id: None,
                        email: email.clone(),
                        active: true,
                    })
                    .await?;
                // Visible inside the same transaction
                assert!(repo.find_by_id(&created.id.unwrap()).await?.is_some());
                Ok::<_, RepoError>(())
            }
        })
        .await;
    assert!(res.is_ok(), "transaction should commit");

    // After commit, a fresh repository should see the row
    let repo_fresh = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;
    let found = repo_fresh
        .find_by_field("email", storeit_core::ParamValue::String(email))
        .await?;
    assert_eq!(found.len(), 1);
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_find_by_id_not_found_returns_none() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;

    let res = repo.find_by_id(&i64::MAX).await?;
    assert!(res.is_none());
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_update_persists_and_only_target_row_changes() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;

    // Seed two users
    let u1 = repo
        .insert(&tests_common::User {
            id: None,
            email: "u1@example.com".into(),
            active: true,
        })
        .await?;
    let u2 = repo
        .insert(&tests_common::User {
            id: None,
            email: "u2@example.com".into(),
            active: true,
        })
        .await?;

    // Update only u1
    let mut u1_mod = u1.clone();
    u1_mod.active = false;
    u1_mod.email = "u1_updated@example.com".into();
    let _ = repo.update(&u1_mod).await?;

    // Fetch both and verify changes are isolated to u1
    let f1 = repo.find_by_id(&u1.id.unwrap()).await?.unwrap();
    let f2 = repo.find_by_id(&u2.id.unwrap()).await?.unwrap();
    assert_eq!(f1.email, "u1_updated@example.com");
    assert!(!f1.active);
    assert_eq!(f2.email, "u2@example.com");
    assert!(f2.active);

    // Verify find_by_field finds exactly the updated row by new email
    let found = repo
        .find_by_field(
            "email",
            storeit_core::ParamValue::String("u1_updated@example.com".into()),
        )
        .await?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, u1.id);
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_delete_idempotent() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let _lock = acquire_it_lock().await;
    let url = shared_db_url().await;

    let repo = TokioPostgresRepository::<tests_common::User, A>::from_url(
        &url,
        tests_common::User::ID_COLUMN,
        A,
    )
    .await?;

    let created = repo
        .insert(&tests_common::User {
            id: None,
            email: "delete@ex.com".into(),
            active: true,
        })
        .await?;
    let id = created.id.unwrap();

    let first = repo.delete_by_id(&id).await?;
    assert!(first, "first delete should return true");

    let second = repo.delete_by_id(&id).await?;
    assert!(!second, "second delete should return false (idempotent)");

    let after = repo.find_by_id(&id).await?;
    assert!(after.is_none());
    Ok(())
}
