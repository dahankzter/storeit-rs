#[test]
#[ignore = "UI expected outputs in flux during rename; to be updated"]
fn ui_compile_fail() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/fail/entity_missing_id.rs");
    t.compile_fail("tests/ui/fail/entity_duplicate_id.rs");
    t.compile_fail("tests/ui/fail/entity_invalid_meta.rs");
    t.compile_fail("tests/ui/fail/repository_invalid_finders_syntax.rs");
    t.compile_fail("tests/ui/fail/repository_unknown_list.rs");
    t.compile_fail("tests/ui/fail/repository_unknown_nv.rs");
    t.compile_fail("tests/ui/fail/repository_unsupported_attr_format.rs");
    t.compile_fail("tests/ui/fail/repository_unsupported_backend.rs");
    t.compile_fail("tests/ui/fail/repository_unsupported_finder_type.rs");
}
