use storeit_macros::Entity;
use storeit_core::{Fetchable, Identifiable};

#[derive(Entity, Clone, Debug, PartialEq)]
struct Article {
    #[fetch(id)]
    id: Option<i64>,
    title: String,
    // Optional field should be accepted and not appear in INSERT/UPDATE when None (macro metadata focus here)
    subtitle: Option<String>,
}

fn main() {
    // Macro should pluralize table name: "articles"
    assert_eq!(Article::TABLE, "articles");
    assert_eq!(Article::SELECT_COLUMNS, &["id", "title", "subtitle"]);
    // Identifiable::Key should be i64 and id() returns Option
    let a = Article { id: Some(5), title: "t".into(), subtitle: None };
    let id: Option<<Article as Identifiable>::Key> = a.id();
    assert_eq!(id, Some(5));
    let _adapter = ArticleRowAdapter;
    let _ = _adapter;
}