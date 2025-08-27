# storeit_macros

Procedural macros for the storeit repository framework.

What it provides:
- `#[derive(Entity)]`: generates compile-time metadata (table, columns) for your struct and can optionally generate backend-specific RowAdapter impls.
- `#[repository]` macro: generate a typed repository module for a specific backend and entity.

Quick example (see the workspace README for full examples):

```rust
use storeit::Entity;

#[derive(Entity, Clone, Debug)]
pub struct User {
    #[fetch(id)]
    pub id: Option<i64>,
    pub email: String,
    pub active: bool,
}
```

These macros are used by the top-level `storeit` facade; most users will only import from `storeit`.

Links:
- Workspace README and examples: https://github.com/dahankzter/storeit-rs#readme

MSRV: 1.70
License: MIT OR Apache-2.0