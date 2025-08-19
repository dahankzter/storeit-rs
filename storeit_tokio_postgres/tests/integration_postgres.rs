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
    if skip_containers() {
        return false;
    }
    std::process::Command::new("docker")
        .arg("version")
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

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_crud_and_find_by_field() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }

    let node = Postgres::default().start().await;
    let port = node.get_host_port_ipv4(5432).await;
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");

    // Apply migrations with retry
    let client = pg_connect_with_retry(&url).await?;
    client
        .batch_execute(tests_common::migrations::POSTGRES_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;

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
    let node = Postgres::default().start().await;
    let port = node.get_host_port_ipv4(5432).await;
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let client = pg_connect_with_retry(&url).await?;
    client
        .batch_execute(tests_common::migrations::POSTGRES_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;

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
    assert!(
        msg.contains("unique") || msg.contains("duplicate"),
        "unexpected error: {}",
        msg
    );
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn postgres_find_by_field_bool_and_null() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let node = Postgres::default().start().await;
    let port = node.get_host_port_ipv4(5432).await;
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let client = pg_connect_with_retry(&url).await?;
    client
        .batch_execute(tests_common::migrations::POSTGRES_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;
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
    let node = Postgres::default().start().await;
    let port = node.get_host_port_ipv4(5432).await;
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let client = pg_connect_with_retry(&url).await?;
    client
        .batch_execute(tests_common::migrations::POSTGRES_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;
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
    assert!(
        msg.contains("column") || msg.contains("unknown"),
        "unexpected error: {}",
        msg
    );
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
    let node = Postgres::default().start().await;
    let port = node.get_host_port_ipv4(5432).await;
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let client = pg_connect_with_retry(&url).await?;
    client
        .batch_execute(tests_common::migrations::POSTGRES_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;

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
    let node = Postgres::default().start().await;
    let port = node.get_host_port_ipv4(5432).await;
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let client = pg_connect_with_retry(&url).await?;
    client
        .batch_execute(tests_common::migrations::POSTGRES_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;
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
    assert!(
        msg.contains("type")
            || msg.contains("cast")
            || msg.contains("operator")
            || msg.contains("mismatch"),
        "unexpected error: {}",
        msg
    );
    Ok(())
}
