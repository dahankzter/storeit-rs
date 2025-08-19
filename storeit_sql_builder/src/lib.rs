#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
//! Minimal SQL builder helpers that leverage metadata from `#[derive(Fetchable)]`.
//!
//! Feature flags select placeholder style:
//! - `tokio_postgres`: $1, $2, ...
//! - `mysql_async`: ?
//! - `rusqlite`: ?
//! - `libsql`: ?
//!
//! Default (no feature): ?

/// Placeholder representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placeholder {
    Dollar,   // $1, $2, ...
    Question, // ?
}

fn placeholder_style() -> Placeholder {
    #[cfg(feature = "tokio_postgres")]
    return Placeholder::Dollar;

    #[cfg(not(feature = "tokio_postgres"))]
    return Placeholder::Question;
}

fn first_placeholder(ph: Placeholder) -> &'static str {
    match ph {
        Placeholder::Dollar => "$1",
        Placeholder::Question => "?",
    }
}

fn placeholder_n(ph: Placeholder, n: usize) -> String {
    match ph {
        Placeholder::Dollar => format!("${}", n),
        Placeholder::Question => "?".to_string(),
    }
}

/// Build a simple SELECT ... WHERE id = <ph> statement using metadata from `E`.
pub fn select_by_id<E>(id_column: &str) -> String
where
    E: storeit_core::Fetchable,
{
    let cols = E::SELECT_COLUMNS.join(", ");
    let table = E::TABLE;
    let ph = first_placeholder(placeholder_style());
    format!(
        "SELECT {cols} FROM {table} WHERE {id} = {ph}",
        cols = cols,
        table = table,
        id = id_column,
        ph = ph
    )
}

/// Build DELETE ... WHERE id = <ph>
pub fn delete_by_id<E>(id_column: &str) -> String
where
    E: storeit_core::Fetchable,
{
    let table = E::TABLE;
    let ph = first_placeholder(placeholder_style());
    format!(
        "DELETE FROM {table} WHERE {id} = {ph}",
        table = table,
        id = id_column,
        ph = ph
    )
}

/// Build INSERT INTO <table> (<cols>) VALUES (<placeholders>)
/// When feature `tokio_postgres` or `libsql_returning` is enabled, this appends `RETURNING <id_column>`.
pub fn insert<E>(id_column: &str) -> String
where
    E: storeit_core::Fetchable + storeit_core::Insertable,
{
    let cols = E::INSERT_COLUMNS;
    let table = E::TABLE;
    let style = placeholder_style();
    let mut phs: Vec<String> = Vec::with_capacity(cols.len());
    for i in 1..=cols.len() {
        phs.push(placeholder_n(style, i));
    }
    let cols_csv = cols.join(", ");
    let ph_csv = phs.join(", ");

    // Two specialized branches to avoid unused_mut and unused variable warnings under -Dwarnings
    #[cfg(any(feature = "tokio_postgres", feature = "libsql_returning"))]
    {
        let mut sql = format!(
            "INSERT INTO {table} ({cols}) VALUES ({vals})",
            table = table,
            cols = cols_csv,
            vals = ph_csv
        );
        sql.push_str(" RETURNING ");
        sql.push_str(id_column);
        sql
    }

    #[cfg(not(any(feature = "tokio_postgres", feature = "libsql_returning")))]
    {
        let _ = id_column; // silence unused parameter when not returning
        format!(
            "INSERT INTO {table} ({cols}) VALUES ({vals})",
            table = table,
            cols = cols_csv,
            vals = ph_csv
        )
    }
}

/// Build UPDATE <table> SET <col1>=<ph1>, ... WHERE <id>=<phN>
pub fn update_by_id<E>(id_column: &str) -> String
where
    E: storeit_core::Fetchable + storeit_core::Updatable,
{
    let cols = E::UPDATE_COLUMNS;
    let table = E::TABLE;
    let style = placeholder_style();

    let mut assignments = Vec::with_capacity(cols.len());
    for (i, col) in cols.iter().enumerate() {
        let ph = placeholder_n(style, i + 1);
        assignments.push(format!("{col} = {ph}", col = col, ph = ph));
    }
    let where_ph = placeholder_n(style, cols.len() + 1);

    format!(
        "UPDATE {table} SET {set_clause} WHERE {id} = {where_ph}",
        table = table,
        set_clause = assignments.join(", "),
        id = id_column,
        where_ph = where_ph
    )
}

/// Build SELECT <cols> FROM <table>
pub fn select_all<E>() -> String
where
    E: storeit_core::Fetchable,
{
    let cols = E::SELECT_COLUMNS.join(", ");
    let table = E::TABLE;
    format!("SELECT {cols} FROM {table}", cols = cols, table = table)
}

/// Build SELECT ... WHERE <field> = <ph>
pub fn select_by_field<E>(field: &str) -> String
where
    E: storeit_core::Fetchable,
{
    let cols = E::SELECT_COLUMNS.join(", ");
    let table = E::TABLE;
    let ph = first_placeholder(placeholder_style());
    format!(
        "SELECT {cols} FROM {table} WHERE {field} = {ph}",
        cols = cols,
        table = table,
        field = field,
        ph = ph
    )
}

/// Build SELECT ... WHERE <field> IS NULL
pub fn select_by_is_null<E>(field: &str) -> String
where
    E: storeit_core::Fetchable,
{
    let cols = E::SELECT_COLUMNS.join(", ");
    let table = E::TABLE;
    format!(
        "SELECT {cols} FROM {table} WHERE {field} IS NULL",
        cols = cols,
        table = table,
        field = field,
    )
}

/// Build SELECT ... WHERE <field> IS NOT NULL
pub fn select_by_is_not_null<E>(field: &str) -> String
where
    E: storeit_core::Fetchable,
{
    let cols = E::SELECT_COLUMNS.join(", ");
    let table = E::TABLE;
    format!(
        "SELECT {cols} FROM {table} WHERE {field} IS NOT NULL",
        cols = cols,
        table = table,
        field = field,
    )
}

/// Build SELECT ... WHERE <field> IN (<ph1>, <ph2>, ...)
pub fn select_by_in<E>(field: &str, count: usize) -> String
where
    E: storeit_core::Fetchable,
{
    let cols = E::SELECT_COLUMNS.join(", ");
    let table = E::TABLE;
    let style = placeholder_style();
    let mut phs: Vec<String> = Vec::with_capacity(count);
    for i in 1..=count {
        phs.push(placeholder_n(style, i));
    }
    let ph_csv = phs.join(", ");
    format!(
        "SELECT {cols} FROM {table} WHERE {field} IN ({phs})",
        cols = cols,
        table = table,
        field = field,
        phs = ph_csv,
    )
}

/// Build SELECT ... WHERE <field> NOT IN (<ph1>, <ph2>, ...)
pub fn select_by_not_in<E>(field: &str, count: usize) -> String
where
    E: storeit_core::Fetchable,
{
    let cols = E::SELECT_COLUMNS.join(", ");
    let table = E::TABLE;
    let style = placeholder_style();
    let mut phs: Vec<String> = Vec::with_capacity(count);
    for i in 1..=count {
        phs.push(placeholder_n(style, i));
    }
    let ph_csv = phs.join(", ");
    format!(
        "SELECT {cols} FROM {table} WHERE {field} NOT IN ({phs})",
        cols = cols,
        table = table,
        field = field,
        phs = ph_csv,
    )
}

/// Build SELECT with optional ORDER BY, LIMIT, OFFSET
pub fn select_with_pagination<E>(
    order_by: Option<&str>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> String
where
    E: storeit_core::Fetchable,
{
    let mut sql = select_all::<E>();
    if let Some(ob) = order_by {
        if !ob.trim().is_empty() {
            sql.push_str(" ORDER BY ");
            sql.push_str(ob);
        }
    }
    if let Some(l) = limit {
        sql.push_str(" LIMIT ");
        sql.push_str(&l.to_string());
    }
    if let Some(off) = offset {
        sql.push_str(" OFFSET ");
        sql.push_str(&off.to_string());
    }
    sql
}

/// Build SELECT COUNT(*) FROM <table>
pub fn select_count_all<E>() -> String
where
    E: storeit_core::Fetchable,
{
    let table = E::TABLE;
    format!("SELECT COUNT(*) FROM {table}", table = table)
}

/// Build SELECT COUNT(*) FROM <table> WHERE <field> = <ph>
pub fn select_count_by_field<E>(field: &str) -> String
where
    E: storeit_core::Fetchable,
{
    let table = E::TABLE;
    let ph = first_placeholder(placeholder_style());
    format!(
        "SELECT COUNT(*) FROM {table} WHERE {field} = {ph}",
        table = table,
        field = field,
        ph = ph
    )
}

/// Build INSERT INTO <table> (<cols>) VALUES rows*(<placeholders>)
/// This generates multi-row VALUES with correct placeholder numbering for Postgres
/// and '?' placeholders for other backends.
pub fn insert_many<E>(rows: usize, id_column: &str) -> String
where
    E: storeit_core::Fetchable + storeit_core::Insertable,
{
    assert!(rows >= 1, "rows must be >= 1");
    let cols = E::INSERT_COLUMNS;
    let table = E::TABLE;
    let style = placeholder_style();

    // One row placeholders
    let mut phs: Vec<String> = Vec::with_capacity(cols.len());
    for i in 1..=cols.len() {
        phs.push(placeholder_n(style, i));
    }
    let cols_csv = cols.join(", ");

    let mut sql = format!(
        "INSERT INTO {table} ({cols}) VALUES ",
        table = table,
        cols = cols_csv,
    );

    match style {
        Placeholder::Question => {
            // Same '?' tuple repeated per row
            let tuple = format!("({})", phs.join(", "));
            let values = std::iter::repeat(tuple)
                .take(rows)
                .collect::<Vec<_>>()
                .join(", ");
            sql.push_str(&values);
        }
        Placeholder::Dollar => {
            // Need increasing $n across all rows
            let mut counter = 1usize;
            for r in 0..rows {
                if r > 0 {
                    sql.push_str(", ");
                }
                let mut row_placeholders = Vec::with_capacity(cols.len());
                for _ in 0..cols.len() {
                    row_placeholders.push(format!("${}", counter));
                    counter += 1;
                }
                sql.push('(');
                sql.push_str(&row_placeholders.join(", "));
                sql.push(')');
            }
        }
    }

    // Silence unused parameter when not returning on these features
    #[cfg(not(any(feature = "tokio_postgres", feature = "libsql_returning")))]
    {
        let _ = id_column;
    }

    // Append RETURNING id_column when features that already do so are enabled
    #[cfg(any(feature = "tokio_postgres", feature = "libsql_returning"))]
    {
        sql.push_str(" RETURNING ");
        sql.push_str(id_column);
    }

    sql
}

/// Build INSERT ... ON CONFLICT DO UPDATE (Postgres)
/// Generates a statement inserting E::INSERT_COLUMNS and updating those same columns
/// from EXCLUDED on conflict of `conflict_column`.
#[cfg(feature = "tokio_postgres")]
pub fn upsert_pg_on_conflict_do_update<E>(conflict_column: &str, id_column: &str) -> String
where
    E: storeit_core::Fetchable + storeit_core::Insertable,
{
    let cols = E::INSERT_COLUMNS;
    let table = E::TABLE;
    let style = placeholder_style();
    let mut phs: Vec<String> = Vec::with_capacity(cols.len());
    for i in 1..=cols.len() {
        phs.push(placeholder_n(style, i));
    }
    let cols_csv = cols.join(", ");
    let ph_csv = phs.join(", ");

    let mut sql = format!(
        "INSERT INTO {table} ({cols}) VALUES ({vals}) ON CONFLICT ({conflict}) DO UPDATE SET ",
        table = table,
        cols = cols_csv,
        vals = ph_csv,
        conflict = conflict_column,
    );
    let mut assigns = Vec::with_capacity(cols.len());
    for col in cols {
        assigns.push(format!("{col} = EXCLUDED.{col}", col = col));
    }
    sql.push_str(&assigns.join(", "));

    // For Postgres we typically want to return id
    sql.push_str(" RETURNING ");
    sql.push_str(id_column);

    sql
}

/// Build MySQL: INSERT ... ON DUPLICATE KEY UPDATE ...
#[cfg(feature = "mysql_async")]
pub fn upsert_mysql_on_duplicate_key_update<E>() -> String
where
    E: storeit_core::Fetchable + storeit_core::Insertable,
{
    let cols = E::INSERT_COLUMNS;
    let table = E::TABLE;
    let style = placeholder_style();
    let mut phs: Vec<String> = Vec::with_capacity(cols.len());
    for i in 1..=cols.len() {
        phs.push(placeholder_n(style, i));
    }
    let cols_csv = cols.join(", ");
    let ph_csv = phs.join(", ");

    let mut sql = format!(
        "INSERT INTO {table} ({cols}) VALUES ({vals}) ON DUPLICATE KEY UPDATE ",
        table = table,
        cols = cols_csv,
        vals = ph_csv,
    );
    let mut assigns = Vec::with_capacity(cols.len());
    for col in cols {
        // VALUES(col) is acceptable here for simplicity
        assigns.push(format!("{col} = VALUES({col})", col = col));
    }
    sql.push_str(&assigns.join(", "));
    sql
}

/// Build WHERE clause for simple conjunction (AND) of equality comparisons.
/// Returns ("WHERE <field1> = <ph> AND <field2> = <ph> ...", params_in_order)
pub fn build_where_and(
    params: &[(&str, storeit_core::ParamValue)],
) -> (String, Vec<storeit_core::ParamValue>) {
    if params.is_empty() {
        return (String::new(), Vec::new());
    }
    let ph_style = placeholder_style();
    let mut clauses: Vec<String> = Vec::with_capacity(params.len());
    let mut out_params: Vec<storeit_core::ParamValue> = Vec::with_capacity(params.len());
    for (i, (field, val)) in params.iter().enumerate() {
        let ph = match ph_style {
            Placeholder::Dollar => placeholder_n(ph_style, i + 1),
            Placeholder::Question => placeholder_n(ph_style, 1),
        };
        clauses.push(format!("{} = {}", field, ph));
        out_params.push(val.clone());
    }
    let sql = format!("WHERE {}", clauses.join(" AND "));
    (sql, out_params)
}

/// Build WHERE clause for disjunction (OR) of groups of ANDed equality comparisons.
/// Each inner vector represents one group combined by AND; groups are then OR-ed together.
/// Returns ("WHERE (a = ? AND b = ?) OR (c = ?)", params)
pub fn build_where_or(
    groups: &[Vec<(&str, storeit_core::ParamValue)>],
) -> (String, Vec<storeit_core::ParamValue>) {
    if groups.is_empty() {
        return (String::new(), Vec::new());
    }
    let ph_style = placeholder_style();
    let mut param_index = 1usize;
    let mut out_params: Vec<storeit_core::ParamValue> = Vec::new();
    let mut rendered_groups: Vec<String> = Vec::with_capacity(groups.len());

    for g in groups {
        if g.is_empty() {
            continue;
        }
        let mut parts: Vec<String> = Vec::with_capacity(g.len());
        for (field, val) in g {
            let ph = match ph_style {
                Placeholder::Dollar => {
                    let p = placeholder_n(ph_style, param_index);
                    param_index += 1;
                    p
                }
                Placeholder::Question => placeholder_n(ph_style, 1),
            };
            parts.push(format!("{} = {}", field, ph));
            out_params.push(val.clone());
        }
        rendered_groups.push(format!("({})", parts.join(" AND ")));
    }
    if rendered_groups.is_empty() {
        return (String::new(), out_params);
    }
    let sql = format!("WHERE {}", rendered_groups.join(" OR "));
    (sql, out_params)
}

/// Build SELECT <cols> FROM <table> WHERE <custom>
pub fn select_where<E>(where_sql: &str) -> String
where
    E: storeit_core::Fetchable,
{
    let mut base = select_all::<E>();
    if !where_sql.trim().is_empty() {
        base.push(' ');
        base.push_str(where_sql);
    }
    base
}

/// Keyset pagination helper over the id column. Returns (SQL, params).
/// When `after` is Some(v): uses `WHERE id > v` (or `< v` when ascending=false) and orders accordingly.
/// When `after` is None: omits the comparison and just orders/limits.
pub fn keyset_by_id<E>(
    id_column: &str,
    after: Option<storeit_core::ParamValue>,
    limit: usize,
    ascending: bool,
) -> (String, Vec<storeit_core::ParamValue>)
where
    E: storeit_core::Fetchable,
{
    let mut sql = String::new();
    // SELECT ... FROM ...
    sql.push_str(&select_all::<E>());
    let ph_style = placeholder_style();
    let mut params: Vec<storeit_core::ParamValue> = Vec::new();

    if let Some(val) = after.clone() {
        // WHERE id > ph (asc) or id < ph (desc)
        let cmp = if ascending { ">" } else { "<" };
        let ph = match ph_style {
            Placeholder::Dollar => placeholder_n(ph_style, 1),
            Placeholder::Question => first_placeholder(ph_style).to_string(),
        };
        sql.push_str(&format!(" WHERE {} {} {}", id_column, cmp, ph));
        params.push(val);
    }

    // ORDER BY and LIMIT
    sql.push_str(" ORDER BY ");
    sql.push_str(id_column);
    sql.push_str(if ascending { " ASC" } else { " DESC" });
    sql.push_str(" LIMIT ");
    sql.push_str(&limit.to_string());

    (sql, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use storeit_macros::Entity; // derive macro

    #[derive(Entity)]

    struct User {
        #[fetch(id)]
        id: i64,
        email: String,
    }

    #[test]
    #[cfg(feature = "tokio_postgres")]
    fn test_select_default_pg() {
        let sql = select_by_id::<User>("id");
        assert_eq!(sql, "SELECT id, email FROM users WHERE id = $1");
    }

    #[cfg(not(feature = "tokio_postgres"))]
    fn test_select_default_q() {
        let sql = select_by_id::<User>("id");
        assert_eq!(sql, "SELECT id, email FROM users WHERE id = ?");
    }

    #[test]
    #[cfg(feature = "tokio_postgres")]
    fn test_delete_default_pg() {
        let sql = delete_by_id::<User>("id");
        assert_eq!(sql, "DELETE FROM users WHERE id = $1");
    }

    #[cfg(not(feature = "tokio_postgres"))]
    fn test_delete_default_q() {
        let sql = delete_by_id::<User>("id");
        assert_eq!(sql, "DELETE FROM users WHERE id = ?");
    }

    #[test]
    #[cfg(feature = "tokio_postgres")]
    fn test_insert_default_pg() {
        let sql = insert::<User>("id");
        assert_eq!(sql, "INSERT INTO users (email) VALUES ($1) RETURNING id");
    }

    #[cfg(not(feature = "tokio_postgres"))]
    fn test_insert_default_q() {
        let sql = insert::<User>("id");
        assert_eq!(sql, "INSERT INTO users (email) VALUES (?)");
    }

    #[test]
    #[cfg(all(not(feature = "tokio_postgres"), feature = "libsql_returning"))]
    fn test_insert_with_libsql_returning() {
        // When libsql_returning is enabled (and not Postgres), the builder should append RETURNING id
        let sql = insert::<User>("id");
        assert_eq!(sql, "INSERT INTO users (email) VALUES (?) RETURNING id");
    }

    #[test]
    fn test_update_default() {
        let sql = update_by_id::<User>("id");
        let style = placeholder_style();
        let expected = format!(
            "UPDATE users SET email = {} WHERE id = {}",
            placeholder_n(style, 1),
            placeholder_n(style, 2)
        );
        assert_eq!(sql, expected);
    }

    #[test]
    fn test_select_all_default() {
        let sql = select_all::<User>();
        assert_eq!(sql, "SELECT id, email FROM users");
    }

    #[test]
    fn test_select_by_field_default() {
        let sql = select_by_field::<User>("email");
        let expected = format!(
            "SELECT id, email FROM users WHERE email = {}",
            first_placeholder(placeholder_style())
        );
        assert_eq!(sql, expected);
    }

    #[test]
    fn test_select_by_is_null_default() {
        let sql = select_by_is_null::<User>("email");
        assert_eq!(sql, "SELECT id, email FROM users WHERE email IS NULL");
    }

    #[test]
    fn test_select_by_is_not_null_default() {
        let sql = select_by_is_not_null::<User>("email");
        assert_eq!(sql, "SELECT id, email FROM users WHERE email IS NOT NULL");
    }

    #[test]
    fn test_select_by_in_default() {
        let sql = select_by_in::<User>("id", 3);
        let style = placeholder_style();
        let phs = vec![
            placeholder_n(style, 1),
            placeholder_n(style, 2),
            placeholder_n(style, 3),
        ]
        .join(", ");
        let expected = format!("SELECT id, email FROM users WHERE id IN ({})", phs);
        assert_eq!(sql, expected);
    }

    #[test]
    fn test_select_by_not_in_default() {
        let sql = select_by_not_in::<User>("id", 2);
        let style = placeholder_style();
        let phs = vec![placeholder_n(style, 1), placeholder_n(style, 2)].join(", ");
        let expected = format!("SELECT id, email FROM users WHERE id NOT IN ({})", phs);
        assert_eq!(sql, expected);
    }

    #[test]
    fn test_select_with_pagination_none() {
        let sql = select_with_pagination::<User>(None, None, None);
        assert_eq!(sql, "SELECT id, email FROM users");
    }

    #[test]
    fn test_select_with_pagination_full() {
        let sql = select_with_pagination::<User>(Some("email DESC"), Some(10), Some(20));
        assert_eq!(
            sql,
            "SELECT id, email FROM users ORDER BY email DESC LIMIT 10 OFFSET 20"
        );
    }

    #[test]
    fn test_select_with_pagination_order_by_only() {
        let sql = select_with_pagination::<User>(Some("email ASC"), None, None);
        assert_eq!(sql, "SELECT id, email FROM users ORDER BY email ASC");
    }

    #[test]
    fn test_select_with_pagination_order_by_empty_only_ignored() {
        let sql = select_with_pagination::<User>(Some(""), None, None);
        assert_eq!(sql, "SELECT id, email FROM users");
    }

    #[test]
    fn test_select_with_pagination_order_by_whitespace_only_ignored() {
        let sql = select_with_pagination::<User>(Some("   \t"), None, None);
        assert_eq!(sql, "SELECT id, email FROM users");
    }

    #[test]
    fn test_select_with_pagination_order_by_empty_ignored() {
        let sql = select_with_pagination::<User>(Some("   \t"), Some(5), Some(0));
        assert_eq!(sql, "SELECT id, email FROM users LIMIT 5 OFFSET 0");
    }

    #[test]
    fn test_select_with_pagination_limit_only() {
        let sql = select_with_pagination::<User>(None, Some(7), None);
        assert_eq!(sql, "SELECT id, email FROM users LIMIT 7");
    }

    #[test]
    fn test_select_with_pagination_offset_only() {
        let sql = select_with_pagination::<User>(None, None, Some(42));
        assert_eq!(sql, "SELECT id, email FROM users OFFSET 42");
    }

    #[test]
    fn test_placeholder_helpers_cover_branches() {
        // Ensure both match arms are covered regardless of active feature flags.
        assert_eq!(first_placeholder(Placeholder::Question), "?");
        assert_eq!(first_placeholder(Placeholder::Dollar), "$1");
        assert_eq!(placeholder_n(Placeholder::Question, 99), "?");
        assert_eq!(placeholder_n(Placeholder::Dollar, 3), "$3");
    }

    // Additional entity to cover custom table and column mapping and placeholder numbering across multiple fields.
    #[derive(Entity)]
    #[allow(dead_code)]
    #[entity(table = "people")]
    struct Person {
        #[fetch(id)]
        id: i64,
        #[fetch(column = "email_address")]
        email: String,
        #[fetch(column = "full_name")]
        name: String,
    }

    #[test]
    fn test_custom_table_and_columns_select_all() {
        let sql = select_all::<Person>();
        assert_eq!(sql, "SELECT id, email_address, full_name FROM people");
    }

    #[test]
    fn test_custom_table_and_columns_insert_and_update() {
        let insert_sql = insert::<Person>("id");
        let style = placeholder_style();
        let mut expected_insert = format!(
            "INSERT INTO people (email_address, full_name) VALUES ({}, {})",
            placeholder_n(style, 1),
            placeholder_n(style, 2)
        );
        if matches!(style, Placeholder::Dollar) {
            expected_insert.push_str(" RETURNING id");
        }
        assert_eq!(insert_sql, expected_insert);

        let update_sql = update_by_id::<Person>("id");
        let expected_update = format!(
            "UPDATE people SET email_address = {}, full_name = {} WHERE id = {}",
            placeholder_n(style, 1),
            placeholder_n(style, 2),
            placeholder_n(style, 3)
        );
        assert_eq!(update_sql, expected_update);
    }

    #[test]
    fn test_select_with_pagination_order_by_and_limit_only() {
        let sql = select_with_pagination::<User>(Some("id DESC"), Some(3), None);
        assert_eq!(sql, "SELECT id, email FROM users ORDER BY id DESC LIMIT 3");
    }

    #[test]
    fn test_select_with_pagination_order_by_and_offset_only() {
        let sql = select_with_pagination::<User>(Some("id ASC"), None, Some(9));
        assert_eq!(sql, "SELECT id, email FROM users ORDER BY id ASC OFFSET 9");
    }

    #[test]
    fn test_select_count_all_default() {
        #[derive(Entity)]
        struct User2 {
            #[fetch(id)]
            id: i64,
            email: String,
        }
        let sql = select_count_all::<User2>();
        assert_eq!(sql, "SELECT COUNT(*) FROM user2s");
    }

    #[test]
    #[cfg(feature = "tokio_postgres")]
    fn test_select_count_by_field_pg() {
        #[derive(Entity)]
        struct User3 {
            #[fetch(id)]
            id: i64,
            email: String,
        }
        let sql = select_count_by_field::<User3>("email");
        assert_eq!(sql, "SELECT COUNT(*) FROM user3s WHERE email = $1");
    }

    #[test]
    #[cfg(not(feature = "tokio_postgres"))]
    fn test_select_count_by_field_q() {
        #[derive(Entity)]
        struct User3 {
            #[fetch(id)]
            id: i64,
            email: String,
        }
        let sql = select_count_by_field::<User3>("email");
        assert_eq!(sql, "SELECT COUNT(*) FROM user3s WHERE email = ?");
    }

    #[test]
    #[cfg(feature = "tokio_postgres")]
    fn test_insert_many_pg_numbering_and_returning() {
        // Use Person (2 insert columns) to exercise numbering across multiple rows
        let sql = insert_many::<Person>(3, "id");
        assert_eq!(sql, "INSERT INTO people (email_address, full_name) VALUES ($1, $2), ($3, $4), ($5, $6) RETURNING id");
    }

    #[test]
    #[cfg(not(feature = "tokio_postgres"))]
    fn test_insert_many_q_placeholders() {
        let sql = insert_many::<Person>(2, "id");
        assert_eq!(
            sql,
            "INSERT INTO people (email_address, full_name) VALUES (?, ?), (?, ?)"
        );
    }

    #[test]
    fn test_derive_paramvalue_for_portable_types() {
        use chrono::{NaiveDate, NaiveDateTime};
        use rust_decimal::Decimal;
        use storeit_core::ParamValue;
        use uuid::Uuid;

        #[derive(Entity)]
        struct TypesEntity {
            #[fetch(id)]
            id: i64,
            dt: NaiveDateTime,
            d: NaiveDate,
            dec: Decimal,
            uid: Uuid,
            opt_dt: Option<NaiveDateTime>,
            opt_uid: Option<Uuid>,
        }

        let e = TypesEntity {
            id: 1,
            dt: NaiveDate::from_ymd_opt(2020, 1, 2)
                .unwrap()
                .and_hms_opt(3, 4, 5)
                .unwrap(),
            d: NaiveDate::from_ymd_opt(2020, 1, 2).unwrap(),
            dec: Decimal::new(12345, 3), // 12.345
            uid: Uuid::nil(),
            opt_dt: None,
            opt_uid: Some(Uuid::nil()),
        };

        let ins = <TypesEntity as storeit_core::Insertable>::insert_values(&e);
        // Expect 6 values (all except id)
        assert_eq!(ins.len(), 6);
        // Spot-check that string variants are produced for new types
        match &ins[0] {
            ParamValue::String(_) => {}
            _ => panic!("expected String for NaiveDateTime"),
        }
        match &ins[1] {
            ParamValue::String(_) => {}
            _ => panic!("expected String for NaiveDate"),
        }
        match &ins[2] {
            ParamValue::String(_) => {}
            _ => panic!("expected String for Decimal"),
        }
        match &ins[3] {
            ParamValue::String(s) => assert!(!s.is_empty()),
            _ => panic!("expected String for Uuid"),
        }
        match &ins[4] {
            ParamValue::Null => {}
            _ => panic!("expected Null for Option<NaiveDateTime> None"),
        }
        match &ins[5] {
            ParamValue::String(s) => assert!(!s.is_empty()),
            _ => panic!("expected String for Option<Uuid> Some"),
        }

        let upd = <TypesEntity as storeit_core::Updatable>::update_values(&e);
        // update_values includes the ID as the last param
        assert_eq!(upd.len(), 7);
        match upd.last().unwrap() {
            ParamValue::I64(v) => assert_eq!(*v, 1),
            _ => panic!("expected last to be id i64"),
        }
    }

    #[test]
    #[cfg(feature = "tokio_postgres")]
    fn test_upsert_pg_on_conflict_do_update_single_col() {
        #[derive(Entity)]
        struct UserU {
            #[fetch(id)]
            id: i64,
            email: String,
        }
        let sql = upsert_pg_on_conflict_do_update::<UserU>("email", "id");
        assert_eq!(sql, "INSERT INTO user_us (email) VALUES ($1) ON CONFLICT (email) DO UPDATE SET email = EXCLUDED.email RETURNING id");
    }

    #[test]
    #[cfg(feature = "tokio_postgres")]
    fn test_upsert_pg_on_conflict_do_update_multi_col() {
        #[derive(Entity)]
        #[entity(table = "people")]
        struct PersonU {
            #[fetch(id)]
            id: i64,
            #[fetch(column = "email_address")]
            email: String,
            #[fetch(column = "full_name")]
            name: String,
        }
        let sql = upsert_pg_on_conflict_do_update::<PersonU>("email_address", "id");
        assert_eq!(sql, "INSERT INTO people (email_address, full_name) VALUES ($1, $2) ON CONFLICT (email_address) DO UPDATE SET email_address = EXCLUDED.email_address, full_name = EXCLUDED.full_name RETURNING id");
    }

    #[test]
    fn test_build_where_and_default() {
        #[derive(Entity)]
        struct U {
            #[fetch(id)]
            id: i64,
            email: String,
            active: bool,
        }
        let (where_sql, params) = build_where_and(&[
            ("email", storeit_core::ParamValue::String("a@x".into())),
            ("active", storeit_core::ParamValue::Bool(true)),
        ]);
        let style = placeholder_style();
        let expected = match style {
            Placeholder::Dollar => format!(
                "WHERE email = {} AND active = {}",
                placeholder_n(style, 1),
                placeholder_n(style, 2)
            ),
            Placeholder::Question => "WHERE email = ? AND active = ?".to_string(),
        };
        assert_eq!(where_sql, expected);
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_build_where_or_default() {
        #[derive(Entity)]
        struct U2 {
            #[fetch(id)]
            id: i64,
            email: String,
            active: bool,
        }
        let groups = vec![
            vec![("email", storeit_core::ParamValue::String("a@x".into()))],
            vec![("active", storeit_core::ParamValue::Bool(true))],
        ];
        let (where_sql, params) = build_where_or(&groups);
        let style = placeholder_style();
        let expected = match style {
            Placeholder::Dollar => format!(
                "WHERE ({}) OR ({})",
                format!("email = {}", placeholder_n(style, 1)),
                format!("active = {}", placeholder_n(style, 2))
            ),
            Placeholder::Question => "WHERE (email = ?) OR (active = ?)".to_string(),
        };
        assert_eq!(where_sql, expected);
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_select_where_wraps() {
        #[derive(Entity)]
        struct U3 {
            #[fetch(id)]
            id: i64,
            email: String,
        }
        let sql = select_where::<U3>("WHERE email = ?");
        assert!(sql.starts_with("SELECT id, email FROM u3s WHERE email"));
    }

    #[test]
    fn test_keyset_by_id_default() {
        #[derive(Entity)]
        struct U4 {
            #[fetch(id)]
            id: i64,
            email: String,
        }
        let (sql, params) =
            keyset_by_id::<U4>("id", Some(storeit_core::ParamValue::I64(10)), 25, true);
        let style = placeholder_style();
        let ph = match style {
            Placeholder::Dollar => placeholder_n(style, 1),
            Placeholder::Question => "?".to_string(),
        };
        assert_eq!(
            sql,
            format!(
                "SELECT id, email FROM u4s WHERE id > {} ORDER BY id ASC LIMIT 25",
                ph
            )
        );
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_keyset_by_id_none_cursor_desc() {
        #[derive(Entity)]
        struct U5 {
            #[fetch(id)]
            id: i64,
        }
        let (sql, params) = keyset_by_id::<U5>("id", None, 5, false);
        assert_eq!(sql, "SELECT id FROM u5s ORDER BY id DESC LIMIT 5");
        assert!(params.is_empty());
    }

    #[test]
    #[cfg(feature = "mysql_async")]
    fn test_upsert_mysql_on_duplicate_key_update() {
        #[derive(Entity)]
        struct UserM {
            #[fetch(id)]
            id: i64,
            email: String,
        }
        let sql = upsert_mysql_on_duplicate_key_update::<UserM>();
        let expected = match placeholder_style() {
            Placeholder::Dollar => "INSERT INTO user_ms (email) VALUES ($1) ON DUPLICATE KEY UPDATE email = VALUES(email)".to_string(),
            Placeholder::Question => "INSERT INTO user_ms (email) VALUES (?) ON DUPLICATE KEY UPDATE email = VALUES(email)".to_string(),
        };
        assert_eq!(sql, expected);
    }
}
