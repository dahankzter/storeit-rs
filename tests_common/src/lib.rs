#![allow(unexpected_cfgs)]
//! Common integration testing utilities and generic tests reusable across backends.

use async_trait::async_trait;
use storeit::Entity;
use storeit_core::Repository;

#[derive(Entity, Clone, Debug, PartialEq)]
#[entity(table = "users")] // consistent across backends
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

/// Expose migration SQL via constants for harnesses.
pub mod migrations {
    pub const POSTGRES_USERS_SQL: &str = include_str!("../migrations/postgres/001_users.sql");
    pub const MYSQL_USERS_SQL: &str = include_str!("../migrations/mysql/001_users.sql");
    pub const LIBSQL_USERS_SQL: &str = include_str!("../migrations/libsql/001_users.sql");
}

#[async_trait]
pub trait RepoFactory {
    /// Construct a clean repository connected to a DB with the required schema.
    async fn new_user_repo(
        &self,
    ) -> storeit_core::RepoResult<Box<dyn Repository<User> + Send + Sync>>;
}

/// Generic CRUD roundtrip test.
pub async fn test_crud_roundtrip<F: RepoFactory + Sync>(f: &F) -> storeit_core::RepoResult<()> {
    let repo = f.new_user_repo().await?;

    let u = User {
        id: None,
        email: "a@example.com".to_string(),
        active: true,
    };
    let created = repo.insert(&u).await?;
    assert!(created.id.is_some());

    let fetched = repo.find_by_id(&created.id.unwrap()).await?;
    assert_eq!(
        fetched.as_ref().map(|x| &x.email),
        Some(&"a@example.com".to_string())
    );

    let mut updated = fetched.unwrap();
    updated.active = false;
    let updated2 = repo.update(&updated).await?;
    assert!(!updated2.active);

    let ok = repo.delete_by_id(&updated2.id.unwrap()).await?;
    assert!(ok);
    Ok(())
}

/// Generic find_by_field test.
pub async fn test_find_by_field<F: RepoFactory + Sync>(f: &F) -> storeit_core::RepoResult<()> {
    let repo = f.new_user_repo().await?;
    let u = User {
        id: None,
        email: "b@example.com".into(),
        active: true,
    };
    let _ = repo.insert(&u).await?;

    let found = repo
        .find_by_field(
            "email",
            storeit_core::ParamValue::String("b@example.com".into()),
        )
        .await?;
    assert_eq!(found.len(), 1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use storeit_core::Fetchable;

    #[test]
    fn user_metadata_constants() {
        // Simple assertions to ensure this crate's entity and re-exports are exercised
        assert_eq!(User::TABLE, "users");
        assert_eq!(User::SELECT_COLUMNS, &["id", "email", "active"]);
    }
}
