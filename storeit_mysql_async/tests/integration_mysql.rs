#![cfg(feature = "mysql-async")]

use storeit_core::Identifiable;
use storeit_core::{RepoError, RepoResult, Repository, RowAdapter};
use storeit_mysql_async::MysqlAsyncRepository;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;

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
    conn.query_drop(tests_common::migrations::MYSQL_USERS_SQL)
        .await
        .map_err(RepoError::backend)?;
    pool.disconnect().await.ok();
    Ok(())
}

/// Try to apply migration with small retries to accommodate server startup time.
async fn apply_migration_with_retry(url: &str) -> RepoResult<()> {
    for _ in 0..10 {
        if apply_migration(url).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    Err(RepoError::backend(std::io::Error::new(
        std::io::ErrorKind::Other,
        "migration failed after retries",
    )))
}

/// Construct a working MySQL connection URL by trying a few common credential combos
/// used by popular MySQL container images.
async fn mysql_url_from_node(
    node: &testcontainers::ContainerAsync<testcontainers_modules::mysql::Mysql>,
) -> RepoResult<String> {
    let host = "127.0.0.1";
    let port: u16 = node.get_host_port_ipv4(3306).await;
    // (user, pass, db)
    let candidates: &[(&str, &str, &str)] = &[
        ("mysql", "mysql", "mysql"),
        ("root", "root", "mysql"),
        ("root", "", "mysql"),
        ("test", "test", "mysql"),
    ];
    for (user, pass, db) in candidates {
        let url = if pass.is_empty() {
            format!("mysql://{user}@{host}:{port}/{db}")
        } else {
            format!("mysql://{user}:{pass}@{host}:{port}/{db}")
        };
        // Probe connectivity
        if mysql_async::Pool::new(url.as_str())
            .get_conn()
            .await
            .is_ok()
        {
            return Ok(url);
        }
    }
    Err(RepoError::backend(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Unable to connect to MySQL container with known credentials",
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

    let node = Mysql::default().start().await;

    // Build a working URL by probing common credentials and wait for readiness
    let url = mysql_url_from_node(&node).await?;
    apply_migration_with_retry(&url).await?;

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
    let node = Mysql::default().start().await;
    let url = mysql_url_from_node(&node).await?;
    apply_migration_with_retry(&url).await?;
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
    assert!(
        msg.contains("duplicate") || msg.contains("unique"),
        "unexpected error: {}",
        msg
    );
    Ok(())
}

#[ignore]
#[tokio::test(flavor = "multi_thread")]
async fn mysql_find_by_field_bool_and_null() -> RepoResult<()> {
    if !containers_usable() {
        eprintln!("[integration] Skipping: Docker not available");
        return Ok(());
    }
    let node = Mysql::default().start().await;
    let url = mysql_url_from_node(&node).await?;
    apply_migration_with_retry(&url).await?;
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
    let node = Mysql::default().start().await;
    let url = mysql_url_from_node(&node).await?;
    apply_migration_with_retry(&url).await?;
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
    assert!(
        msg.contains("unknown") || msg.contains("column"),
        "unexpected error: {}",
        msg
    );
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
    let node = Mysql::default().start().await;
    let url = mysql_url_from_node(&node).await?;
    apply_migration_with_retry(&url).await?;

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
    let node = Mysql::default().start().await;
    let url = mysql_url_from_node(&node).await?;
    apply_migration_with_retry(&url).await?;
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
