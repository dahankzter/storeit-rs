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
    use storeit::Identifiable;
    use storeit_core::Fetchable;

    #[test]
    fn user_metadata_constants() {
        // Simple assertions to ensure this crate's entity and re-exports are exercised
        assert_eq!(User::TABLE, "users");
        assert_eq!(User::SELECT_COLUMNS, &["id", "email", "active"]);
    }

    #[test]
    fn migrations_constants_non_empty() {
        // Ensure migrations constants are referenced so their lines are covered
        let pg = migrations::POSTGRES_USERS_SQL;
        let my = migrations::MYSQL_USERS_SQL;
        let ls = migrations::LIBSQL_USERS_SQL;
        assert!(pg.contains("CREATE TABLE") && pg.contains("users"));
        assert!(my.contains("CREATE TABLE") && my.contains("users"));
        assert!(ls.contains("CREATE TABLE") && ls.contains("users"));
        // Also touch some additional entity metadata to improve coverage stability
        let _id_col = User::ID_COLUMN;
        let _sel = User::SELECT_COLUMNS;
    }
}

#[cfg(test)]
mod in_memory_repo_tests {
    use super::*;
    use storeit_core::{ParamValue, RepoResult};

    #[derive(Default)]
    struct MemState {
        rows: Vec<User>,
        next_id: i64,
    }

    #[derive(Clone, Default)]
    struct MemRepo {
        state: std::sync::Arc<std::sync::Mutex<MemState>>,
    }

    #[async_trait]
    impl Repository<User> for MemRepo {
        async fn find_by_id(
            &self,
            id: &<User as storeit_core::Identifiable>::Key,
        ) -> RepoResult<Option<User>> {
            let g = self.state.lock().unwrap();
            Ok(g.rows.iter().find(|u| u.id == Some(*id)).cloned())
        }

        async fn find_by_field(
            &self,
            field_name: &str,
            value: ParamValue,
        ) -> RepoResult<Vec<User>> {
            let g = self.state.lock().unwrap();
            let v: Vec<User> = match (field_name, value) {
                ("email", ParamValue::String(s)) => {
                    g.rows.iter().cloned().filter(|u| u.email == s).collect()
                }
                ("active", ParamValue::Bool(b)) => {
                    g.rows.iter().cloned().filter(|u| u.active == b).collect()
                }
                _ => Vec::new(),
            };
            Ok(v)
        }

        async fn insert(&self, entity: &User) -> RepoResult<User> {
            let mut g = self.state.lock().unwrap();
            if g.next_id == 0 {
                g.next_id = 1;
            }
            let mut e = entity.clone();
            e.id = Some(g.next_id);
            g.next_id += 1;
            g.rows.push(e.clone());
            Ok(e)
        }

        async fn update(&self, entity: &User) -> RepoResult<User> {
            let mut g = self.state.lock().unwrap();
            if let Some(id) = entity.id {
                if let Some(row) = g.rows.iter_mut().find(|u| u.id == Some(id)) {
                    row.email = entity.email.clone();
                    row.active = entity.active;
                }
            }
            Ok(entity.clone())
        }

        async fn delete_by_id(
            &self,
            id: &<User as storeit_core::Identifiable>::Key,
        ) -> RepoResult<bool> {
            let mut g = self.state.lock().unwrap();
            let before = g.rows.len();
            g.rows.retain(|u| u.id != Some(*id));
            Ok(g.rows.len() != before)
        }
    }

    struct MemFactory;

    #[async_trait]
    impl RepoFactory for MemFactory {
        async fn new_user_repo(
            &self,
        ) -> storeit_core::RepoResult<Box<dyn Repository<User> + Send + Sync>> {
            Ok(Box::new(MemRepo::default()))
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn generic_helpers_run_with_in_memory_repo() -> storeit_core::RepoResult<()> {
        let f = MemFactory;
        test_crud_roundtrip(&f).await?;
        test_find_by_field(&f).await?;
        Ok(())
    }
}
