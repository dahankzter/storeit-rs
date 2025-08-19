#![allow(unexpected_cfgs)]
use storeit::*;

#[derive(Entity, Clone, Debug, PartialEq)]
struct Mini {
    #[fetch(id)]
    id: Option<i64>,
    #[fetch(column = "email_address")]
    email: String,
}

#[test]
fn facade_reexports_and_entity_metadata() {
    // Ensure re-exported traits and macros are usable from the facade crate.
    assert_eq!(Mini::TABLE, "minis");
    assert_eq!(Mini::SELECT_COLUMNS, &["id", "email_address"]);

    // Ensure the generated RowAdapter type is in scope via facade usage.
    let _adapter = MiniRowAdapter;
    let _ = _adapter;

    // Exercise core types through facade.
    let v = vec![
        ParamValue::String("a".into()),
        ParamValue::I32(1),
        ParamValue::I64(2),
        ParamValue::F64(3.0),
        ParamValue::Bool(true),
        ParamValue::Null,
    ];
    assert_eq!(v.len(), 6);
}
