#![forbid(unsafe_code)]
#![cfg_attr(
    not(feature = "libsql-backend"),
    doc = "Enable feature `libsql-backend` to use this adapter."
)]

#[cfg(feature = "libsql-backend")]
mod backend {
    use std::cell::RefCell;
    use std::sync::Arc;
    use std::time::Instant;
    use storeit_core::transactions::{
        Isolation, Propagation, TransactionContext, TransactionDefinition, TransactionManager,
    };

    #[cfg(feature = "tracing")]
    use tracing::info;

    #[inline]
    #[allow(unused_variables)]
    fn obs_record(op: &str, table: &str, start: Instant, rows: usize, success: bool) {
        let elapsed = start.elapsed().as_millis() as u64;
        #[cfg(feature = "tracing")]
        {
            info!(
                sql_kind = "sql",
                table = table,
                op = op,
                rows = rows,
                elapsed_ms = elapsed,
                success = success,
                "repo op"
            );
        }
        #[cfg(feature = "metrics")]
        {
            metrics::counter!("repo_ops_total", 1, "op" => op.to_string(), "table" => table.to_string(), "success" => success.to_string());
            metrics::histogram!("repo_op_duration_ms", elapsed as f64, "op" => op.to_string(), "table" => table.to_string());
            if !success {
                metrics::counter!("repo_op_errors_total", 1, "op" => op.to_string(), "table" => table.to_string());
            }
        }
    }

    // Task-local state for current transaction connection and savepoint depth.
    tokio::task_local! {
        static TX_STACK: RefCell<Vec<libsql::Connection>>;
        static SP_DEPTH: RefCell<usize>;
    }

    fn begin_sql(isolation: Isolation) -> &'static str {
        match isolation {
            Isolation::Default | Isolation::ReadCommitted => "BEGIN DEFERRED",
            Isolation::RepeatableRead => "BEGIN IMMEDIATE",
            Isolation::Serializable => "BEGIN EXCLUSIVE",
        }
    }

    /// A concrete TransactionManager for libsql/SQLite.
    #[derive(Clone)]
    pub struct LibsqlTransactionManager {
        db: Arc<Database>,
    }

    impl LibsqlTransactionManager {
        pub fn new(db: Arc<Database>) -> Self {
            Self { db }
        }
        pub fn from_arc(db: Arc<Database>) -> Self {
            Self { db }
        }

        /// Vend a repository bound to the current transaction connection if available,
        /// otherwise a regular repository against the manager's database.
        pub async fn repository<T, A>(
            &self,
            _ctx: TransactionContext<'_>,
            adapter: A,
        ) -> storeit_core::RepoResult<LibsqlRepository<T, A>>
        where
            T: Fetchable + Identifiable + Insertable + Updatable + 'static,
            A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
        {
            let conn_opt = TX_STACK
                .try_with(|cell| cell.borrow().last().cloned())
                .ok()
                .flatten();
            Ok(match conn_opt {
                Some(conn) => LibsqlRepository::from_conn(self.db.clone(), conn, adapter),
                None => LibsqlRepository::new(self.db.clone(), adapter),
            })
        }
    }

    #[async_trait::async_trait]
    impl TransactionManager for LibsqlTransactionManager {
        async fn execute<'a, R, F, Fut>(
            &'a self,
            def: &TransactionDefinition,
            f: F,
        ) -> storeit_core::RepoResult<R>
        where
            F: FnOnce(TransactionContext<'a>) -> Fut + Send + 'a,
            Fut: core::future::Future<Output = storeit_core::RepoResult<R>> + Send + 'a,
            R: Send + 'a,
        {
            // Define the core logic as an async block so we can run it inside task-local scopes when needed.
            let fut = async {
                let mut created_tx = false;
                let mut used_savepoint = false;

                let active = TX_STACK
                    .try_with(|cell| !cell.borrow().is_empty())
                    .unwrap_or(false);

                if matches!(
                    def.propagation,
                    Propagation::NotSupported | Propagation::Supports
                ) && !active
                {
                    return f(TransactionContext::new()).await;
                }
                if matches!(def.propagation, Propagation::Never) && active {
                    return Err(storeit_core::RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Transaction exists but Propagation::Never requested",
                    )));
                }

                let conn = if active {
                    TX_STACK.with(|cell| cell.borrow().last().cloned().expect("stack non-empty"))
                } else {
                    self.db
                        .connect()
                        .map_err(storeit_core::RepoError::backend)?
                };

                if !active {
                    if def.read_only {
                        conn.execute("PRAGMA query_only = ON", ()).await.ok();
                    }
                    // Apply a busy_timeout to reduce spurious SQLITE_BUSY during tests. Use provided timeout or a small default.
                    let busy_ms = def.timeout.map(|d| d.as_millis() as i64).unwrap_or(1000);
                    conn.execute(&format!("PRAGMA busy_timeout = {}", busy_ms), ())
                        .await
                        .ok();
                    conn.execute(begin_sql(def.isolation), ())
                        .await
                        .map_err(storeit_core::RepoError::backend)?;
                    TX_STACK.with(|cell| cell.borrow_mut().push(conn.clone()));
                    SP_DEPTH.with(|d| *d.borrow_mut() = 0);
                    created_tx = true;
                } else {
                    match def.propagation {
                        Propagation::RequiresNew | Propagation::Nested => {
                            let depth = SP_DEPTH.with(|d| *d.borrow());
                            let name = format!("sp{}", depth + 1);
                            conn.execute(&format!("SAVEPOINT {}", name), ()).await.ok();
                            SP_DEPTH.with(|d| *d.borrow_mut() += 1);
                            used_savepoint = true;
                        }
                        Propagation::Required | Propagation::Supports => {}
                        Propagation::NotSupported => {}
                        Propagation::Never => {}
                    }
                }

                let result = f(TransactionContext::new()).await;

                if created_tx {
                    if result.is_ok() {
                        conn.execute("COMMIT", ())
                            .await
                            .map_err(storeit_core::RepoError::backend)?;
                    } else {
                        conn.execute("ROLLBACK", ())
                            .await
                            .map_err(storeit_core::RepoError::backend)?;
                    }
                    if def.read_only {
                        conn.execute("PRAGMA query_only = OFF", ()).await.ok();
                    }
                    TX_STACK.with(|cell| {
                        let _ = cell.borrow_mut().pop();
                    });
                } else if used_savepoint {
                    let name = SP_DEPTH.with(|d| {
                        let v = *d.borrow();
                        format!("sp{}", v)
                    });
                    if result.is_ok() {
                        conn.execute(&format!("RELEASE SAVEPOINT {}", name), ())
                            .await
                            .ok();
                    } else {
                        conn.execute(&format!("ROLLBACK TO SAVEPOINT {}", name), ())
                            .await
                            .ok();
                    }
                    SP_DEPTH.with(|d| {
                        let mut b = d.borrow_mut();
                        if *b > 0 {
                            *b -= 1;
                        }
                    });
                }

                result
            };

            // If the task-local TX_STACK isn't initialized for this task, set up scopes and run.
            let not_initialized = TX_STACK.try_with(|_| ()).is_err();
            if not_initialized {
                TX_STACK
                    .scope(RefCell::new(Vec::new()), async move {
                        SP_DEPTH.scope(RefCell::new(0usize), fut).await
                    })
                    .await
            } else {
                fut.await
            }
        }
    }
    use async_trait::async_trait;
    use libsql::{params, Database, Row, Value};
    use std::collections::HashMap;
    use std::marker::PhantomData;
    use std::sync::Mutex;
    use storeit_core::{
        Fetchable, Identifiable, Insertable, ParamValue, RepoError, RepoResult, Repository,
        RowAdapter, Updatable,
    };

    // Helper function to convert ParamValue to libsql::Value.
    fn to_libsql_value(p: ParamValue) -> Value {
        match p {
            ParamValue::String(s) => s.into(),
            ParamValue::I32(i) => (i as i64).into(), // libsql uses i64 for integers
            ParamValue::I64(i) => i.into(),
            ParamValue::F64(f) => f.into(),
            ParamValue::Bool(b) => (b as i64).into(), // SQLite bools are 0/1
            ParamValue::Null => Value::Null,
        }
    }

    /// A fully asynchronous, `libsql`-backed repository.
    struct RepoSql<T> {
        select_by_id: String,
        delete_by_id: String,
        insert: String,
        update_by_id: String,
        find_by_field_cache: Mutex<HashMap<String, String>>,
        _marker: PhantomData<T>,
    }

    impl<T> RepoSql<T>
    where
        T: Fetchable + Identifiable + Insertable + Updatable,
    {
        fn new() -> Self {
            let select_by_id = storeit_sql_builder::select_by_id::<T>(T::ID_COLUMN);
            let delete_by_id = storeit_sql_builder::delete_by_id::<T>(T::ID_COLUMN);
            let insert = storeit_sql_builder::insert::<T>(T::ID_COLUMN);
            let update_by_id = storeit_sql_builder::update_by_id::<T>(T::ID_COLUMN);
            Self {
                select_by_id,
                delete_by_id,
                insert,
                update_by_id,
                find_by_field_cache: Mutex::new(HashMap::new()),
                _marker: PhantomData,
            }
        }

        fn get_select_by_field(&self, field: &str) -> String
        where
            T: Fetchable,
        {
            let mut guard = self.find_by_field_cache.lock().unwrap();
            if let Some(s) = guard.get(field) {
                return s.clone();
            }
            let built = storeit_sql_builder::select_by_field::<T>(field);
            guard.insert(field.to_string(), built.clone());
            built
        }
    }

    /// A fully asynchronous, `libsql`-backed repository.
    pub struct LibsqlRepository<T, A>
    where
        T: Identifiable + 'static,
        A: RowAdapter<T> + Send + Sync + 'static,
    {
        db: Arc<Database>,
        /// Optional connection bound to a transaction context. When set, all operations
        /// will use this connection instead of opening a new one.
        conn: Option<libsql::Connection>,
        adapter: A,
        sql: RepoSql<T>,
        _marker: PhantomData<T>,
    }

    impl<T, A> LibsqlRepository<T, A>
    where
        T: Identifiable + 'static,
        A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
    {
        /// Creates a new repository from an existing `libsql::Database` object.
        pub fn new(db: Arc<Database>, adapter: A) -> Self
        where
            T: Fetchable + Identifiable + Insertable + Updatable,
        {
            let sql = RepoSql::<T>::new();
            Self {
                db,
                conn: None,
                adapter,
                sql,
                _marker: PhantomData,
            }
        }

        /// Creates a new repository from an existing connection. All operations
        /// will execute on the provided connection (useful for transaction-bound repos).
        pub fn from_conn(db: Arc<Database>, conn: libsql::Connection, adapter: A) -> Self
        where
            T: Fetchable + Identifiable + Insertable + Updatable,
        {
            let sql = RepoSql::<T>::new();
            Self {
                db,
                conn: Some(conn),
                adapter,
                sql,
                _marker: PhantomData,
            }
        }

        /// Creates a new repository by connecting to a database URL.
        pub async fn from_url(
            database_url: &str,
            _id_column: &str, // Note: id_column is now read from T::ID_COLUMN
            adapter: A,
        ) -> RepoResult<Self>
        where
            T: Fetchable + Identifiable + Insertable + Updatable,
        {
            // Database::open is deprecated upstream; keep a narrow allow here until Builder migration
            #[allow(deprecated)]
            let db = Arc::new(Database::open(database_url).map_err(RepoError::backend)?);
            Ok(Self::new(db, adapter))
        }
    }

    #[async_trait]
    impl<T, A> Repository<T> for LibsqlRepository<T, A>
    where
        T: Fetchable + Identifiable + Insertable + Updatable + Send + Sync + Clone + 'static,
        A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
        T::Key: Clone
            + Send
            + Sync
            + 'static
            + Default
            + PartialEq
            + Into<libsql::Value>
            + serde::Serialize
            + serde::de::DeserializeOwned,
    {
        async fn find_by_id(&self, id: &T::Key) -> RepoResult<Option<T>> {
            let __start = Instant::now();
            // Prefer an active transaction-bound connection if present in task-local storage.
            let conn = if let Ok(Some(tx_conn)) =
                TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                tx_conn
            } else if let Some(c) = &self.conn {
                c.clone()
            } else {
                self.db.connect().map_err(RepoError::backend)?
            };
            let mut rows = conn
                .query(&self.sql.select_by_id, params!(id.clone()))
                .await
                .map_err(RepoError::backend)?;

            if let Ok(Some(row)) = rows.next().await {
                let entity = self.adapter.from_row(&row)?;
                obs_record("find_by_id", T::TABLE, __start, 1, true);
                Ok(Some(entity))
            } else {
                obs_record("find_by_id", T::TABLE, __start, 0, true);
                Ok(None)
            }
        }

        async fn find_by_field(&self, field_name: &str, value: ParamValue) -> RepoResult<Vec<T>> {
            let __start = Instant::now();
            let sql = self.sql.get_select_by_field(field_name);
            let value_param = to_libsql_value(value);
            // Prefer an active transaction-bound connection if present in task-local storage.
            let conn = if let Ok(Some(tx_conn)) =
                TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                tx_conn
            } else if let Some(c) = &self.conn {
                c.clone()
            } else {
                self.db.connect().map_err(RepoError::backend)?
            };
            let mut rows = conn
                .query(&sql, params!(value_param))
                .await
                .map_err(RepoError::backend)?;

            let mut entities = Vec::new();
            while let Ok(Some(row)) = rows.next().await {
                entities.push(self.adapter.from_row(&row)?);
            }
            let len = entities.len();
            obs_record("find_by_field", T::TABLE, __start, len, true);
            Ok(entities)
        }

        async fn insert(&self, entity: &T) -> RepoResult<T> {
            let __start = Instant::now();
            let values: Vec<Value> = entity
                .insert_values()
                .into_iter()
                .map(to_libsql_value)
                .collect();
            // Prefer an active transaction-bound connection if present in task-local storage.
            let conn = if let Ok(Some(tx_conn)) =
                TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                tx_conn
            } else if let Some(c) = &self.conn {
                c.clone()
            } else {
                self.db.connect().map_err(RepoError::backend)?
            };
            #[cfg(feature = "libsql_returning")]
            {
                // Use INSERT ... RETURNING to obtain the new id
                let mut rows = conn
                    .query(&self.sql.insert, values)
                    .await
                    .map_err(RepoError::backend)?;
                let row = rows
                    .next()
                    .await
                    .map_err(RepoError::backend)?
                    .ok_or_else(|| {
                        RepoError::backend(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "no row returned from INSERT ... RETURNING",
                        ))
                    })?;
                let ret_id: i64 = row.get(0).map_err(RepoError::backend)?;
                let new_key: T::Key = serde_json::from_value(serde_json::Value::from(ret_id))
                    .map_err(RepoError::backend)?;
                // Fetch using the same connection to avoid any visibility issues
                let mut rows2 = conn
                    .query(&self.sql.select_by_id, params!(new_key.clone().into()))
                    .await
                    .map_err(RepoError::backend)?;
                if let Ok(Some(row2)) = rows2.next().await {
                    let out = self.adapter.from_row(&row2);
                    if out.is_ok() {
                        obs_record("insert", T::TABLE, __start, 1, true);
                    } else {
                        obs_record("insert", T::TABLE, __start, 0, false);
                    }
                    return out;
                } else {
                    obs_record("insert", T::TABLE, __start, 0, false);
                    return Err(RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Failed to fetch entity after insert",
                    )));
                }
            }

            #[cfg(not(feature = "libsql_returning"))]
            {
                conn.execute(&self.sql.insert, values)
                    .await
                    .map_err(RepoError::backend)?;

                let new_id = conn.last_insert_rowid();
                let new_key: T::Key = serde_json::from_value(serde_json::Value::from(new_id))
                    .map_err(RepoError::backend)?;

                // Fetch using the same connection to avoid any visibility issues
                let mut rows2 = conn
                    .query(&self.sql.select_by_id, params!(new_key.clone().into()))
                    .await
                    .map_err(RepoError::backend)?;
                if let Ok(Some(row2)) = rows2.next().await {
                    let out = self.adapter.from_row(&row2);
                    if out.is_ok() {
                        obs_record("insert", T::TABLE, __start, 1, true);
                    } else {
                        obs_record("insert", T::TABLE, __start, 0, false);
                    }
                    out
                } else {
                    obs_record("insert", T::TABLE, __start, 0, false);
                    return Err(RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Failed to fetch entity after insert",
                    )));
                }
            }
        }

        async fn update(&self, entity: &T) -> RepoResult<T> {
            let __start = Instant::now();
            let values: Vec<Value> = entity
                .update_values()
                .into_iter()
                .map(to_libsql_value)
                .collect();
            // Prefer an active transaction-bound connection if present in task-local storage.
            let conn = if let Ok(Some(tx_conn)) =
                TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                tx_conn
            } else if let Some(c) = &self.conn {
                c.clone()
            } else {
                self.db.connect().map_err(RepoError::backend)?
            };
            conn.execute(&self.sql.update_by_id, values)
                .await
                .map_err(RepoError::backend)?;
            obs_record("update", T::TABLE, __start, 1, true);
            Ok(entity.clone())
        }

        async fn delete_by_id(&self, id: &T::Key) -> RepoResult<bool> {
            let __start = Instant::now();
            // Prefer an active transaction-bound connection if present in task-local storage.
            let conn = if let Ok(Some(tx_conn)) =
                TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                tx_conn
            } else if let Some(c) = &self.conn {
                c.clone()
            } else {
                self.db.connect().map_err(RepoError::backend)?
            };
            let n = conn
                .execute(&self.sql.delete_by_id, params!(id.clone()))
                .await
                .map_err(RepoError::backend)?;
            let ok = n > 0;
            obs_record("delete_by_id", T::TABLE, __start, n as usize, true);
            Ok(ok)
        }
    }
}

#[cfg(feature = "libsql-backend")]
pub use backend::{LibsqlRepository, LibsqlTransactionManager};

#[cfg(all(test, feature = "libsql-backend"))]
mod tests {
    use super::backend::{LibsqlRepository, LibsqlTransactionManager};
    use libsql::Database;
    use std::sync::{Arc, OnceLock};
    use storeit_core::transactions::{
        Isolation, Propagation, TransactionDefinition, TransactionManager,
    };
    use storeit_core::{Repository, RowAdapter};
    use tokio::sync::Mutex as AsyncMutex;

    #[derive(storeit_macros::Entity, Clone, Debug, PartialEq)]
    #[entity(table = "users")]
    struct U {
        #[fetch(id)]
        id: Option<i64>,
        email: String,
        active: bool,
    }

    struct A;
    impl RowAdapter<U> for A {
        type Row = libsql::Row;
        fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<U> {
            let id: i64 = row.get(0).map_err(storeit_core::RepoError::mapping)?;
            let email: String = row.get(1).map_err(storeit_core::RepoError::mapping)?;
            let active: i64 = row.get(2).map_err(storeit_core::RepoError::mapping)?;
            Ok(U {
                id: Some(id),
                email,
                active: active != 0,
            })
        }
    }

    static DB_INIT: OnceLock<AsyncMutex<()>> = OnceLock::new();
    async fn setup_db() -> Arc<Database> {
        // Serialize DB setup across tests to avoid libsql file locking edge-cases.
        let _guard = DB_INIT.get_or_init(|| AsyncMutex::new(())).lock().await;

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Place test databases under the system temp directory
        let tmp_dir = std::env::temp_dir();
        let path = tmp_dir.join(format!("storeit_libsql_tests_{}.sqlite3", ts));
        // Database::open is deprecated upstream; narrow allow inside tests setup only.
        #[allow(deprecated)]
        let db = Database::open(format!("file:{}?mode=rwc", path.display())).expect("open db");
        let conn = db.connect().expect("connect");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY AUTOINCREMENT, email TEXT NOT NULL UNIQUE, active INTEGER NOT NULL);",
            (),
        )
        .await
        .expect("apply schema");
        Arc::new(db)
        // _guard dropped here at end of function scope
    }

    #[tokio::test]
    async fn find_by_field_with_unknown_column_surfaces_query_error() {
        let db = setup_db().await;
        let repo: LibsqlRepository<U, A> = LibsqlRepository::new(db.clone(), A);
        // Use a non-existent column name to cause a SQL error at execution
        let err = repo
            .find_by_field(
                "does_not_exist",
                storeit_core::ParamValue::String("x".into()),
            )
            .await
            .expect_err("expected query to fail");
        let msg = format!("{:#}", err);
        let lower = msg.to_lowercase();
        assert!(
            lower.contains("no such column")
                || lower.contains("backend error")
                || lower.contains("failed"),
            "unexpected error: {}",
            msg
        );
    }

    // Adapter that intentionally requests a missing column index to force a mapping error
    struct BadAdapter;
    impl RowAdapter<U> for BadAdapter {
        type Row = libsql::Row;
        fn from_row(&self, row: &Self::Row) -> storeit_core::RepoResult<U> {
            // Try to read a non-existent column index to trigger an error
            let _: String = row.get(999).map_err(storeit_core::RepoError::mapping)?;
            unreachable!("should have failed before");
        }
    }

    #[tokio::test]
    async fn row_adapter_mapping_error_surfaces() {
        let db = setup_db().await;
        // Use a good repo to insert a row
        let good: LibsqlRepository<U, A> = LibsqlRepository::new(db.clone(), A);
        let created = good
            .insert(&U {
                id: None,
                email: "map@x".into(),
                active: true,
            })
            .await
            .expect("insert ok");

        // Now construct a repo with a bad adapter that will fail during mapping
        let bad: LibsqlRepository<U, BadAdapter> = LibsqlRepository::new(db.clone(), BadAdapter);
        let _err = bad
            .find_by_id(&created.id.unwrap())
            .await
            .expect_err("expected mapping error");
        // Any error is acceptable; this path ensures RowAdapter failures propagate as errors.
    }

    // Non-ignored regression test: a prebuilt repository created outside the transaction
    // should automatically participate in the active transaction (via task-local pickup),
    // and committed changes should be visible afterwards.
    #[tokio::test]
    async fn transaction_repository_reuse_commits() {
        let db = setup_db().await;
        let mgr = LibsqlTransactionManager::from_arc(db.clone());
        // Prebuild a repository OUTSIDE any transaction and reuse it inside.
        let repo_outside: LibsqlRepository<U, A> = LibsqlRepository::new(db.clone(), A);

        let def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        };
        let email = format!(
            "reuse_{}@x",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        // Execute a transaction and use the prebuilt repository inside it
        let res = mgr
            .execute(&def, |_ctx| {
                let repo = repo_outside; // move into async block
                let email = email.clone();
                async move {
                    let created = repo
                        .insert(&U {
                            id: None,
                            email: email.clone(),
                            active: true,
                        })
                        .await?;
                    // Visible inside the same transaction
                    assert!(repo.find_by_id(&created.id.unwrap()).await?.is_some());
                    Ok::<_, storeit_core::RepoError>(())
                }
            })
            .await;
        assert!(res.is_ok(), "transaction should commit: {:?}", res);

        // After commit, a fresh repository (new connection) should see the row
        let repo_fresh: LibsqlRepository<U, A> = LibsqlRepository::new(db.clone(), A);
        let found = repo_fresh
            .find_by_field("email", storeit_core::ParamValue::String(email.clone()))
            .await
            .expect("query after commit");
        assert_eq!(found.len(), 1, "expected one row visible after commit");
    }

    #[tokio::test]
    #[ignore = "libsql tx manager WIP - skip in default test runs"]
    async fn transaction_commit_persists() {
        let db = setup_db().await;
        let mgr = LibsqlTransactionManager::from_arc(db.clone());
        let def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        };
        let mgr2 = mgr.clone();
        mgr2.execute(&def, |ctx| async move {
            let repo: LibsqlRepository<U, A> = mgr.repository(ctx, A).await?;
            let u1 = U {
                id: None,
                email: "a@x".into(),
                active: true,
            };
            let u2 = U {
                id: None,
                email: "b@x".into(),
                active: false,
            };
            let u1 = repo.insert(&u1).await?;
            let u2 = repo.insert(&u2).await?;
            // inner visibility
            assert!(repo.find_by_id(&u1.id.unwrap()).await?.is_some());
            assert!(repo.find_by_id(&u2.id.unwrap()).await?.is_some());
            Ok::<_, storeit_core::RepoError>(())
        })
        .await
        .expect("tx execute");

        // After commit, new connection sees data
        let repo2: LibsqlRepository<U, A> = LibsqlRepository::new(db.clone(), A);
        let found = repo2
            .find_by_field("email", storeit_core::ParamValue::String("a@x".into()))
            .await
            .expect("query");
        assert_eq!(found.len(), 1);
    }

    #[tokio::test]
    #[ignore = "libsql tx manager WIP - skip in default test runs"]
    async fn transaction_rollback_on_error() {
        let db = setup_db().await;
        let mgr = LibsqlTransactionManager::new(db.clone());
        let def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        };
        let mgr2 = mgr.clone();
        let err = mgr2
            .execute::<(), _, _>(&def, |ctx| async move {
                let repo: LibsqlRepository<U, A> = mgr.repository(ctx, A).await?;
                let u1 = U {
                    id: None,
                    email: "c@x".into(),
                    active: true,
                };
                let _ = repo.insert(&u1).await?;
                Err::<(), storeit_core::RepoError>(storeit_core::RepoError::backend(
                    std::io::Error::new(std::io::ErrorKind::Other, "boom"),
                ))
            })
            .await
            .expect_err("should rollback");
        let _ = err; // silence unused
        let repo2: LibsqlRepository<U, A> = LibsqlRepository::new(db, A);
        let found = repo2
            .find_by_field("email", storeit_core::ParamValue::String("c@x".into()))
            .await
            .expect("query");
        assert!(found.is_empty());
    }

    #[tokio::test]
    #[ignore = "libsql tx manager WIP - skip in default test runs"]
    async fn propagation_requires_new_savepoint_isolated() {
        let db = setup_db().await;
        let mgr = LibsqlTransactionManager::new(db.clone());
        let outer_def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        };
        let outer_mgr = mgr.clone();
        outer_mgr
            .clone()
            .execute(&outer_def, |ctx_outer| async move {
                let repo: LibsqlRepository<U, A> = mgr.repository(ctx_outer, A).await?;
                let u_outer = U {
                    id: None,
                    email: "outer@x".into(),
                    active: true,
                };
                let u_outer = repo.insert(&u_outer).await?;

                let inner_def = TransactionDefinition {
                    propagation: Propagation::RequiresNew,
                    isolation: Isolation::Default,
                    read_only: false,
                    timeout: None,
                };
                let inner_mgr = outer_mgr.clone();
                let _ = inner_mgr
                    .execute::<(), _, _>(&inner_def, |ctx_inner| async move {
                        let repo_inner: LibsqlRepository<U, A> =
                            mgr.repository(ctx_inner, A).await?;
                        let u_inner = U {
                            id: None,
                            email: "inner@x".into(),
                            active: false,
                        };
                        let _ = repo_inner.insert(&u_inner).await?;
                        Err::<(), storeit_core::RepoError>(storeit_core::RepoError::backend(
                            std::io::Error::new(std::io::ErrorKind::Other, "inner fails"),
                        ))
                    })
                    .await
                    .expect_err("inner should rollback");

                // Outer still sees its insert
                assert!(repo.find_by_id(&u_outer.id.unwrap()).await?.is_some());
                Ok::<_, storeit_core::RepoError>(())
            })
            .await
            .expect("outer ok");

        let repo2: LibsqlRepository<U, A> = LibsqlRepository::new(db, A);
        let outer = repo2
            .find_by_field("email", storeit_core::ParamValue::String("outer@x".into()))
            .await
            .expect("query");
        let inner = repo2
            .find_by_field("email", storeit_core::ParamValue::String("inner@x".into()))
            .await
            .expect("query");
        assert_eq!(outer.len(), 1);
        assert_eq!(inner.len(), 0);
    }

    #[tokio::test]
    #[ignore = "libsql tx manager WIP - skip in default test runs"]
    async fn propagation_nested_savepoint_rollback_does_not_affect_outer() {
        let db = setup_db().await;
        let mgr = LibsqlTransactionManager::new(db.clone());
        let outer_def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        };
        let outer_mgr = mgr.clone();
        outer_mgr
            .clone()
            .execute(&outer_def, |ctx_outer| async move {
                let repo: LibsqlRepository<U, A> = mgr.repository(ctx_outer, A).await?;
                let u_outer = U {
                    id: None,
                    email: "outer2@x".into(),
                    active: true,
                };
                let u_outer = repo.insert(&u_outer).await?;

                let inner_def = TransactionDefinition {
                    propagation: Propagation::Nested,
                    isolation: Isolation::Default,
                    read_only: false,
                    timeout: None,
                };
                let inner_mgr = outer_mgr.clone();
                let _ = inner_mgr
                    .execute::<(), _, _>(&inner_def, |ctx_inner| async move {
                        let repo_inner: LibsqlRepository<U, A> =
                            mgr.repository(ctx_inner, A).await?;
                        let u_inner = U {
                            id: None,
                            email: "inner2@x".into(),
                            active: false,
                        };
                        let _ = repo_inner.insert(&u_inner).await?;
                        Err::<(), storeit_core::RepoError>(storeit_core::RepoError::backend(
                            std::io::Error::new(std::io::ErrorKind::Other, "inner fails"),
                        ))
                    })
                    .await
                    .expect_err("inner should rollback");

                // Outer still sees its insert
                assert!(repo.find_by_id(&u_outer.id.unwrap()).await?.is_some());
                Ok::<_, storeit_core::RepoError>(())
            })
            .await
            .expect("outer ok");

        let repo2: LibsqlRepository<U, A> = LibsqlRepository::new(db.clone(), A);
        let outer = repo2
            .find_by_field("email", storeit_core::ParamValue::String("outer2@x".into()))
            .await
            .expect("query");
        let inner = repo2
            .find_by_field("email", storeit_core::ParamValue::String("inner2@x".into()))
            .await
            .expect("query");
        assert_eq!(outer.len(), 1);
        assert_eq!(inner.len(), 0);
    }

    #[tokio::test]
    #[ignore = "libsql tx manager WIP - skip in default test runs"]
    async fn read_only_enforced_best_effort() {
        let db = setup_db().await;
        let mgr = LibsqlTransactionManager::new(db.clone());
        let def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: true,
            timeout: None,
        };
        let mgr2 = mgr.clone();
        let err = mgr2
            .execute(&def, |ctx| async move {
                let repo: LibsqlRepository<U, A> = mgr.repository(ctx, A).await?;
                let u = U {
                    id: None,
                    email: "ro@x".into(),
                    active: true,
                };
                let _ = repo.insert(&u).await?;
                Ok::<_, storeit_core::RepoError>(())
            })
            .await
            .expect_err("writes should be blocked in read-only");
        let _ = err;
    }

    #[tokio::test]
    #[ignore = "libsql tx manager WIP - skip in default test runs"]
    async fn timeout_best_effort() {
        // Test busy_timeout is applied: create a writer lock and then attempt another write with short timeout
        let db = setup_db().await;
        // Establish a writer that holds a transaction
        let conn1 = db.connect().expect("connect1");
        conn1
            .execute("BEGIN IMMEDIATE", ())
            .await
            .expect("begin immediate");
        conn1
            .execute("INSERT INTO users (email, active) VALUES ('lock@x', 1)", ())
            .await
            .expect("insert");

        let mgr = LibsqlTransactionManager::new(db.clone());
        let def = TransactionDefinition {
            propagation: Propagation::Required,
            isolation: Isolation::RepeatableRead,
            read_only: false,
            timeout: Some(std::time::Duration::from_millis(1)),
        };
        let mgr2 = mgr.clone();
        let res = mgr2
            .execute(&def, |ctx| async move {
                let repo: LibsqlRepository<U, A> = mgr.repository(ctx, A).await?;
                let u = U {
                    id: None,
                    email: "timeout@x".into(),
                    active: true,
                };
                let _ = repo.insert(&u).await?;
                Ok::<_, storeit_core::RepoError>(())
            })
            .await;
        assert!(res.is_err(), "expected busy/timeout error due to lock");

        // cleanup
        conn1.execute("ROLLBACK", ()).await.ok();
    }
}
