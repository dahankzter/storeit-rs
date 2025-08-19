// Criterion benches for basic CRUD operations using libsql (SQLite) in-memory.
// Run locally with:
//   cargo bench -p storeit_libsql --features libsql-backend --bench ops

#![allow(unexpected_cfgs)]

#[cfg(feature = "libsql-backend")]
mod bench_impl {
    use criterion::{black_box, BatchSize, Criterion};
    use storeit_core::Repository;

    #[derive(storeit_macros::Entity, Clone, Debug, PartialEq)]
    #[entity(table = "users")]
    struct U {
        #[fetch(id)]
        id: Option<i64>,
        email: String,
        active: bool,
    }

    struct A;
    impl storeit_core::RowAdapter<U> for A {
        type Row = libsql::Row;
        fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<U> {
            let id: i64 = row.get(0).map_err(storeit_core::RepoError::mapping)?;
            let email: String = row.get(1).map_err(storeit_core::RepoError::mapping)?;
            let active_i64: i64 = row.get(2).map_err(storeit_core::RepoError::mapping)?;
            Ok(U {
                id: Some(id),
                email,
                active: active_i64 != 0,
            })
        }
    }

    fn setup_repo() -> storeit_core::RepoResult<storeit_libsql::LibsqlRepository<U, A>> {
        // Shared in-memory database with basic schema.
        #[allow(deprecated)]
        let db = libsql::Database::open("file::memory:?cache=shared").expect("open db");
        let conn = db.connect().expect("connect");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS users (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  email TEXT NOT NULL UNIQUE,\n  active INTEGER NOT NULL\n);",
                (),
            )
            .await
            .expect("apply schema");
        });
        Ok(storeit_libsql::LibsqlRepository::new(
            std::sync::Arc::new(db),
            A,
        ))
    }

    pub fn bench_insert(c: &mut Criterion) {
        let mut group = c.benchmark_group("libsql_insert");
        group.bench_function("insert_unique", |b| {
            b.iter_batched(
                || setup_repo().expect("repo"),
                |repo| {
                    // unique email to avoid UNIQUE constraint violation
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos();
                    let u = U {
                        id: None,
                        email: format!("i_{ts}@x"),
                        active: true,
                    };
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let created = rt
                        .block_on(async { repo.insert(&u).await })
                        .expect("insert ok");
                    black_box(created);
                },
                BatchSize::SmallInput,
            )
        });
        group.finish();
    }

    pub fn bench_find_update_delete(c: &mut Criterion) {
        let mut group = c.benchmark_group("libsql_find_update_delete");

        group.bench_function("find_by_id", |b| {
            b.iter_batched(
                || {
                    let repo = setup_repo().expect("repo");
                    // seed one row
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let u = U {
                        id: None,
                        email: "seed@x".into(),
                        active: true,
                    };
                    let created = rt.block_on(async { repo.insert(&u).await }).expect("seed");
                    (repo, created.id.unwrap())
                },
                |(repo, id)| {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let found = rt
                        .block_on(async { repo.find_by_id(&id).await })
                        .expect("find");
                    black_box(found);
                },
                BatchSize::SmallInput,
            )
        });

        group.bench_function("update_toggle", |b| {
            b.iter_batched(
                || {
                    let repo = setup_repo().expect("repo");
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let u = U {
                        id: None,
                        email: "upd@x".into(),
                        active: true,
                    };
                    let created = rt.block_on(async { repo.insert(&u).await }).expect("seed");
                    (repo, created)
                },
                |(repo, mut row)| {
                    row.active = !row.active;
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let updated = rt
                        .block_on(async { repo.update(&row).await })
                        .expect("update");
                    black_box(updated);
                },
                BatchSize::SmallInput,
            )
        });

        group.bench_function("insert_and_delete", |b| {
            b.iter_batched(
                || setup_repo().expect("repo"),
                |repo| {
                    // Insert a row and then delete it within the same iteration
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos();
                    let mut u = U {
                        id: None,
                        email: format!("d_{ts}@x"),
                        active: true,
                    };
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let created = rt
                        .block_on(async { repo.insert(&u).await })
                        .expect("insert");
                    u.id = created.id;
                    let ok = rt
                        .block_on(async { repo.delete_by_id(&u.id.unwrap()).await })
                        .expect("delete");
                    black_box(ok);
                },
                BatchSize::SmallInput,
            )
        });

        group.finish();
    }
}

// Define the Criterion entry points at the crate root so `main` exists at crate level.
#[cfg(feature = "libsql-backend")]
use bench_impl::{bench_find_update_delete, bench_insert};
#[cfg(feature = "libsql-backend")]
criterion::criterion_group!(benches, bench_insert, bench_find_update_delete);
#[cfg(feature = "libsql-backend")]
criterion::criterion_main!(benches);

// Fallback when feature is not enabled: provide a dummy main so the bench binary compiles.
#[cfg(not(feature = "libsql-backend"))]
fn main() {
    eprintln!("Enable feature libsql-backend to run benches.");
}
