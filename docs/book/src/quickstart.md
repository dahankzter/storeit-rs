# Quick Start

Note on crate names and aliasing: The facade crate is named `storeit`. The examples below use `repository::...` by aliasing the dependency in your Cargo.toml. Add this to your project to follow along:

```toml
[dependencies]
# Local path usage during development
repository = { package = "storeit", path = "./storeit" }
# Or, from crates.io (when published)
# repository = { package = "storeit", version = "0.1" }
```

The examples under the facade crate demonstrate the canonical flow. Below is an overview; see repository/examples/ for runnable versions per backend.

```rust
use repository::{Repository as _, RowAdapter};

#[derive(repository::Entity, Clone, Debug, PartialEq)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}

// Minimal RowAdapter example (libsql)
struct UserAdapter;
impl RowAdapter<User> for UserAdapter {
    type Row = libsql::Row;
    fn from_row(&self, row: &Self::Row) -> repository::RepoResult<User> {
        let id: i64 = row.get(0)?;
        let email: String = row.get(1)?;
        let active_i64: i64 = row.get(2)?;
        Ok(User { id: Some(id), email, active: active_i64 != 0 })
    }
}

#[repository::repository(entity = User, backend = Libsql, finders(find_by_email: String))]
pub mod users_repo {}
```

- See `examples/README.md` for how to run.
- For connection and pooling tips, read the guide below.
