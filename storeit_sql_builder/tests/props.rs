#![allow(unexpected_cfgs)]

use proptest::prelude::*;
use storeit_macros::Entity;

#[test]
fn cover_placeholder_variants() {
    // Ensure both variants are referenced so the enum variants are considered constructed
    let _d = Placeholder::Dollar;
    let _q = Placeholder::Question;
}

#[derive(Entity)]
#[entity(table = "people_props")]
struct PersonP {
    #[fetch(id)]
    id: i64,
    #[fetch(column = "email_address")]
    email: String,
    #[fetch(column = "full_name")]
    name: String,
}

proptest! {
    // Property: insert_many placeholders equal rows * columns; numbering continuous in PG.
    #[test]
    fn insert_many_placeholder_count(rows in 1usize..10) {
        let sql = storeit_sql_builder::insert_many::<PersonP>(rows, "id");
        let cols = <PersonP as storeit_core::Insertable>::INSERT_COLUMNS.len();
        let expected = rows * cols;
        match placeholder_style() {
            Placeholder::Question => {
                let count = sql.matches('?').count();
                prop_assert_eq!(count, expected);
            }
            Placeholder::Dollar => {
                for i in 1..=expected {
                                let needle = format!("${}", i);
                                let ok = sql.contains(&needle);
                                prop_assert!(ok);
                            }
            }
        }
    }
}

proptest! {
    // Property: build_where_and/build_where_or placeholder count equals params length.
    #[test]
    fn where_builders_placeholder_count(a in any::<bool>(), b in any::<bool>()) {
        let params = vec![("flag_a", storeit_core::ParamValue::Bool(a)), ("flag_b", storeit_core::ParamValue::Bool(b))];
        let (wa_sql, wa_params) = storeit_sql_builder::build_where_and(&params);
        let (wo_sql, wo_params) = storeit_sql_builder::build_where_or(&vec![vec![("flag_a", storeit_core::ParamValue::Bool(a))], vec![("flag_b", storeit_core::ParamValue::Bool(b))]]);
        match placeholder_style() {
            Placeholder::Question => {
                prop_assert_eq!(wa_sql.matches('?').count(), wa_params.len());
                prop_assert_eq!(wo_sql.matches('?').count(), wo_params.len());
            }
            Placeholder::Dollar => {
                let count_wa = (1..=wa_params.len()).filter(|i| wa_sql.contains(&format!("${}", i))).count();
                let count_wo = (1..=wo_params.len()).filter(|i| wo_sql.contains(&format!("${}", i))).count();
                prop_assert_eq!(count_wa, wa_params.len());
                prop_assert_eq!(count_wo, wo_params.len());
            }
        }
    }
}

// Local helpers mirroring crate-internal types to avoid private visibility issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placeholder {
    Dollar,
    Question,
}

fn placeholder_style() -> Placeholder {
    #[cfg(feature = "tokio_postgres")]
    {
        Placeholder::Dollar
    }
    #[cfg(not(feature = "tokio_postgres"))]
    {
        Placeholder::Question
    }
}
