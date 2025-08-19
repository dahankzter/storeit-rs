//! Procedural macros for the `storeit-rs` repository library.
//!
//! This crate provides two main macros:
//! - `#[derive(Entity)]`: A derive macro that inspects a struct and generates all the
//!   necessary metadata and a default `RowAdapter` implementation for it to be used
//!   in a repository.
//! - `#[repository(...)]`: An attribute macro that generates a complete, asynchronous
//!   repository module for an entity.

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    spanned::Spanned,
    Data, DeriveInput, Fields, Ident, ItemMod, LitStr, Token, Type, TypePath,
};

use inflections::Inflect;

// --- Helper Structs & Functions for Parsing ---

/// A helper struct for parsing `key = "value"` style meta attributes.
struct MetaNameValue {
    pub path: syn::Path,
    pub _eq_token: Token![=],
    pub value: LitStr,
}

impl Parse for MetaNameValue {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            path: input.parse()?,
            _eq_token: input.parse()?,
            value: input.parse()?,
        })
    }
}

/// Helper to check if a type is an `Option<T>`.
fn is_option(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if type_path.qself.is_none() && type_path.path.leading_colon.is_none() {
            if let Some(segment) = type_path.path.segments.last() {
                return segment.ident == "Option";
            }
        }
    }
    false
}

/// Helper to get the inner type of an `Option<T>`.
fn get_option_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        let path = &type_path.path;
        if path.segments.last().is_some_and(|s| s.ident == "Option") {
            if let Some(segment) = path.segments.last() {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner_ty)) = args.args.first() {
                        return Some(inner_ty);
                    }
                }
            }
        }
    }
    None
}

/// Holds parsed metadata about a single struct field.
#[derive(Clone)]
struct FieldMetadata {
    ident: Ident,
    ty: Type,
    ty_str: String,
    column_name: String,
    is_id: bool,
    is_skipped: bool,
}

/// Parses all named fields from a `DeriveInput` struct.
fn parse_field_metadata(input: &DeriveInput) -> Vec<FieldMetadata> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => named,
            _ => panic!("#[derive(Entity)] only supports structs with named fields."),
        },
        _ => panic!("#[derive(Entity)] can only be used on structs."),
    };

    fields
        .named
        .iter()
        .map(|field| {
            let ident = field.ident.as_ref().unwrap().clone();
            let ty = field.ty.clone();
            let ty_str = ty.to_token_stream().to_string().replace(' ', "");
            let mut column_name = ident.to_string();
            let mut is_id = false;
            let mut is_skipped = false;

            for attr in &field.attrs {
                if attr.path().is_ident("fetch") {
                    if let Ok(list) = attr.meta.require_list() {
                        // Propagate parse errors to cause a compile error for invalid meta, e.g., #[fetch(column)]
                        list.parse_nested_meta(|meta| {
                            if meta.path.is_ident("column") {
                                let value = meta
                                    .value()
                                    .expect("Invalid #[fetch(column = \"...\")] syntax");
                                let s: LitStr = value
                                    .parse()
                                    .expect("Invalid #[fetch(column = \"...\")] value");
                                column_name = s.value();
                            } else if meta.path.is_ident("id") {
                                is_id = true;
                            } else if meta.path.is_ident("skip") {
                                is_skipped = true;
                            }
                            Ok(())
                        })
                        .expect("Invalid #[fetch(...)] attribute syntax");
                    }
                }
            }
            FieldMetadata {
                ident,
                ty,
                ty_str,
                column_name,
                is_id,
                is_skipped,
            }
        })
        .collect()
}

// --- `Entity` derive macro ---

#[proc_macro_derive(Entity, attributes(entity, fetch))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;
    let fields_metadata = parse_field_metadata(&input);

    // --- Get table name ---
    // Look for `#[entity(table = "...")]` first.
    let table_name_override = input.attrs.iter().find_map(|attr| {
        if attr.path().is_ident("entity") {
            if let Ok(meta) = attr.meta.require_list() {
                let parsed: Result<MetaNameValue, _> = syn::parse2(meta.tokens.clone());
                if let Ok(MetaNameValue { path, value, .. }) = parsed {
                    if path.is_ident("table") {
                        return Some(value.value());
                    }
                }
            }
        }
        None
    });

    // If no override, deduce it from the struct name (`User` -> `users`).
    let table_name = table_name_override
        .unwrap_or_else(|| format!("{}s", struct_name.to_string().to_snake_case()));

    // Basic validation of table and column names to avoid generating invalid SQL identifiers.
    fn is_valid_ident(s: &str) -> bool {
        let mut chars = s.chars();
        match chars.next() {
            Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
            _ => return false,
        }
        for ch in chars {
            if !(ch == '_' || ch.is_ascii_alphanumeric()) {
                return false;
            }
        }
        true
    }
    if !is_valid_ident(&table_name) {
        panic!("Invalid table name `{}`. Use ASCII letters, digits, or `_`, starting with a letter or `_`. See docs/architecture.md (Entities) for guidance.", table_name);
    }
    for f in &fields_metadata {
        if !f.is_skipped && !is_valid_ident(&f.column_name) {
            panic!("Invalid column name `{}`. Use ASCII letters, digits, or `_`, starting with a letter or `_`. See docs/architecture.md (Entities) for guidance.", f.column_name);
        }
    }

    // --- Implement `Fetchable` ---
    let select_columns: Vec<_> = fields_metadata
        .iter()
        .filter(|f| !f.is_skipped)
        .map(|f| &f.column_name)
        .collect();
    let findable_columns: Vec<_> = fields_metadata
        .iter()
        .filter(|f| {
            !f.is_id
                && !f.is_skipped
                && matches!(f.ty_str.as_str(), "String" | "i32" | "i64" | "f64" | "bool")
        })
        .map(|f| {
            let col = &f.column_name;
            let ty_str = &f.ty_str;
            quote! { (#col, #ty_str) }
        })
        .collect();

    let fetchable_impl = quote! {
        impl ::storeit_core::Fetchable for #struct_name {
            const TABLE: &'static str = #table_name;
            const SELECT_COLUMNS: &'static [&'static str] = &[#(#select_columns),*];
            const FINDABLE_COLUMNS: &'static [(&'static str, &'static str)] = &[#(#findable_columns),*];
        }
    };

    // --- Implement `Identifiable` ---
    // Validate exactly one #[fetch(id)]
    let id_count = fields_metadata.iter().filter(|f| f.is_id).count();
    if id_count == 0 {
        panic!("A field must be marked with #[fetch(id)]. Hint: mark your primary key field like `#[fetch(id)]`. See docs/architecture.md (Entities) for details.");
    } else if id_count > 1 {
        panic!("Exactly one field must be marked with #[fetch(id)] (found {}). Remove extra #[fetch(id)] attributes. See docs/architecture.md (Entities).", id_count);
    }

    let id_field = fields_metadata
        .iter()
        .find(|f| f.is_id)
        .expect("unreachable: validated id_count == 1");
    let id_ident = &id_field.ident;
    let id_ty = &id_field.ty;
    let key_ty = get_option_inner(id_ty).unwrap_or(id_ty);
    let id_column_name = &id_field.column_name;

    let id_accessor = if is_option(id_ty) {
        quote! { self.#id_ident.clone() }
    } else {
        quote! { Some(self.#id_ident.clone()) }
    };

    let identifiable_impl = quote! {
        impl ::storeit_core::Identifiable for #struct_name {
            type Key = #key_ty;
            const ID_COLUMN: &'static str = #id_column_name;
            fn id(&self) -> Option<Self::Key> {
                #id_accessor
            }
        }
    };

    // --- Implement `Insertable` and `Updatable` ---
    let to_param_value = |field: &FieldMetadata| {
        let ident = &field.ident;
        let ty_str = &field.ty_str;

        if is_option(&field.ty) {
            return match ty_str.as_str() {
                s if s.contains("String") => {
                    quote! { self.#ident.as_ref().cloned().map(::storeit_core::ParamValue::String).unwrap_or(::storeit_core::ParamValue::Null) }
                }
                s if s.contains("i32") => {
                    quote! { self.#ident.map_or(::storeit_core::ParamValue::Null, ::storeit_core::ParamValue::I32) }
                }
                s if s.contains("i64") => {
                    quote! { self.#ident.map_or(::storeit_core::ParamValue::Null, ::storeit_core::ParamValue::I64) }
                }
                s if s.contains("f64") => {
                    quote! { self.#ident.map_or(::storeit_core::ParamValue::Null, ::storeit_core::ParamValue::F64) }
                }
                s if s.contains("bool") => {
                    quote! { self.#ident.map_or(::storeit_core::ParamValue::Null, ::storeit_core::ParamValue::Bool) }
                }
                s if s.contains("SystemTime") => {
                    quote! { self.#ident.map_or(::storeit_core::ParamValue::Null, |st| ::storeit_core::ParamValue::I64(::chrono::DateTime::<::chrono::Utc>::from(st).timestamp_millis())) }
                }
                s if s.contains("NaiveDateTime") => {
                    quote! { self.#ident.as_ref().map(|v| ::storeit_core::ParamValue::String(v.to_string())).unwrap_or(::storeit_core::ParamValue::Null) }
                }
                s if s.contains("NaiveDate") => {
                    quote! { self.#ident.as_ref().map(|v| ::storeit_core::ParamValue::String(v.to_string())).unwrap_or(::storeit_core::ParamValue::Null) }
                }
                s if s.contains("rust_decimal::Decimal") || s.ends_with("::Decimal") || s == "Decimal" || s.contains("Decimal") => {
                    quote! { self.#ident.as_ref().map(|v| ::storeit_core::ParamValue::String(v.to_string())).unwrap_or(::storeit_core::ParamValue::Null) }
                }
                s if s.contains("uuid::Uuid") || s.ends_with("::Uuid") || s == "Uuid" || s.contains("Uuid") => {
                    quote! { self.#ident.as_ref().map(|v| ::storeit_core::ParamValue::String(v.to_string())).unwrap_or(::storeit_core::ParamValue::Null) }
                }
                _ => panic!("Unsupported Option type for ParamValue: {}. Hint: map this field to a supported type (String/i32/i64/f64/bool), or mark it with #[fetch(skip)] to exclude it from persistence.", ty_str),
            };
        }

        match ty_str.as_str() {
            "String" => quote! { ::storeit_core::ParamValue::String(self.#ident.clone()) },
            "i32" => quote! { ::storeit_core::ParamValue::I32(self.#ident) },
            "i64" => quote! { ::storeit_core::ParamValue::I64(self.#ident) },
            "f64" => quote! { ::storeit_core::ParamValue::F64(self.#ident) },
            "bool" => quote! { ::storeit_core::ParamValue::Bool(self.#ident) },
            s if s.ends_with("SystemTime") => {
                quote! { ::storeit_core::ParamValue::I64(::chrono::DateTime::<::chrono::Utc>::from(self.#ident).timestamp_millis()) }
            }
            s if s.ends_with("NaiveDateTime") => {
                // Use Display to format to a portable string (e.g., "YYYY-MM-DD HH:MM:SS").
                quote! { ::storeit_core::ParamValue::String(self.#ident.to_string()) }
            }
            s if s.ends_with("NaiveDate") => {
                // Format as YYYY-MM-DD.
                quote! { ::storeit_core::ParamValue::String(self.#ident.to_string()) }
            }
            s if s.ends_with("rust_decimal::Decimal") || s.ends_with("Decimal") => {
                // Decimal string representation.
                quote! { ::storeit_core::ParamValue::String(self.#ident.to_string()) }
            }
            s if s.ends_with("uuid::Uuid") || s.ends_with("Uuid") => {
                // Standard hyphenated UUID string.
                quote! { ::storeit_core::ParamValue::String(self.#ident.to_string()) }
            }
            _ => panic!("Unsupported type for ParamValue: {}. Hint: map this field to a supported type (String/i32/i64/f64/bool) or mark it with #[fetch(skip)]. See docs/plan.md item 15 for portable types.", ty_str),
        }
    };

    let insert_fields: Vec<_> = fields_metadata
        .iter()
        .filter(|f| !f.is_id && !f.is_skipped)
        .collect();
    let insert_columns: Vec<_> = insert_fields.iter().map(|f| &f.column_name).collect();
    let insert_values: Vec<_> = insert_fields.iter().map(|f| to_param_value(f)).collect();

    let insertable_impl = quote! {
        impl ::storeit_core::Insertable for #struct_name {
            const INSERT_COLUMNS: &'static [&'static str] = &[#(#insert_columns),*];
            fn insert_values(&self) -> Vec<::storeit_core::ParamValue> {
                vec![#(#insert_values),*]
            }
        }
    };

    let update_fields: Vec<_> = fields_metadata
        .iter()
        .filter(|f| !f.is_id && !f.is_skipped)
        .collect();
    let update_columns: Vec<_> = update_fields.iter().map(|f| &f.column_name).collect();
    let mut update_values: Vec<_> = update_fields.iter().map(|f| to_param_value(f)).collect();
    update_values.push(to_param_value(id_field));

    let updatable_impl = quote! {
        impl ::storeit_core::Updatable for #struct_name {
            const UPDATE_COLUMNS: &'static [&'static str] = &[#(#update_columns),*];
            fn update_values(&self) -> Vec<::storeit_core::ParamValue> {
                vec![#(#update_values),*]
            }
        }
    };

    // --- Generate `RowAdapter` ---
    let adapter_struct_name = Ident::new(&format!("{}RowAdapter", struct_name), struct_name.span());

    let try_get_mappings: Vec<_> = fields_metadata
        .iter()
        .map(|f| {
            if f.is_skipped {
                let ident = &f.ident;
                return quote! { #ident: ::core::default::Default::default() };
            }
            let ident = &f.ident;
            let col_name_lit = LitStr::new(&f.column_name, ident.span());
            let ty_str = &f.ty_str;
            if ty_str.ends_with("SystemTime") {
                quote! {
                    #ident: {
                        let val: ::chrono::DateTime<::chrono::Utc> = row
                            .try_get(#col_name_lit)
                            .map_err(|e| ::storeit_core::RepoError::mapping(e))?;
                        val.into()
                    }
                }
            } else {
                quote! { #ident: row
                .try_get(#col_name_lit)
                .map_err(|e| ::storeit_core::RepoError::mapping(e))? }
            }
        })
        .collect();

    let mysql_get_mappings: Vec<_> = fields_metadata
        .iter()
        .map(|f| {
            if f.is_skipped {
                let ident = &f.ident;
                return quote! { #ident: ::core::default::Default::default() };
            }
            let ident = &f.ident;
            let col_name_lit = LitStr::new(&f.column_name, ident.span());
            let ty_str = &f.ty_str;

            if ty_str.ends_with("SystemTime") {
                quote! {
                    #ident: {
                        let naive_dt: ::chrono::NaiveDateTime = row.get(#col_name_lit)
                            .ok_or_else(|| ::storeit_core::RepoError::mapping(std::io::Error::new(std::io::ErrorKind::Other, format!("missing required column {}.{} in row", #table_name, #col_name_lit))))?;
                        ::chrono::DateTime::<::chrono::Utc>::from_naive_utc_and_offset(naive_dt, ::chrono::Utc).into()
                    }
                }
            } else {
                quote! { #ident: row.get(#col_name_lit).ok_or_else(|| ::storeit_core::RepoError::mapping(std::io::Error::new(std::io::ErrorKind::Other, format!("missing required column {}.{} in row", #table_name, #col_name_lit))))? }
            }
        })
        .collect();

    let libsql_get_mappings: Vec<_> = fields_metadata
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if f.is_skipped {
                let ident = &f.ident;
                return quote! { #ident: ::core::default::Default::default() };
            }
            let ident = &f.ident;
            let col_index = i as i32;
            let ty_str = &f.ty_str;
            if ty_str.ends_with("SystemTime") {
                quote! {
                    #ident: {
                        // Assumes the DB stores a unix timestamp (i64) and we convert it.
                        let ts_seconds: i64 = row
                            .get(#col_index)
                            .map_err(|e| ::storeit_core::RepoError::mapping(e))?;
                        ::chrono::DateTime::from_timestamp(ts_seconds, 0)
                            .map(std::convert::Into::into)
                            .ok_or_else(|| ::storeit_core::RepoError::mapping(std::io::Error::new(std::io::ErrorKind::Other, format!("Invalid timestamp in {}[{}]", #table_name, #col_index))))?
                    }
                }
            } else {
                quote! { #ident: row
                    .get(#col_index)
                    .map_err(|e| ::storeit_core::RepoError::mapping(e))? }
            }
        })
        .collect();

    let row_adapter_impls = quote! {
        #[derive(Debug, Clone, Copy, Default)]
        pub struct #adapter_struct_name;

        // During coverage runs, cargo-llvm-cov sets cfg(coverage). To keep coverage stable in
        // crates that don't link backend crates directly, we disable backend-specific adapters
        // under cfg(coverage).
        #[cfg(all(feature = "backend-adapters", not(coverage)))]
        impl ::storeit_core::RowAdapter<#struct_name> for #adapter_struct_name {
            type Row = ::tokio_postgres::Row;
            fn from_row(&self, row: &Self::Row) -> ::storeit_core::RepoResult<#struct_name> {
                Ok(#struct_name {
                    #(#try_get_mappings),*
                })
            }
        }

        #[cfg(all(feature = "backend-adapters", not(coverage)))]
        impl ::storeit_core::RowAdapter<#struct_name> for #adapter_struct_name {
            type Row = ::mysql_async::Row;
            fn from_row(&self, row: &Self::Row) -> ::storeit_core::RepoResult<#struct_name> {
                Ok(#struct_name {
                    #(#mysql_get_mappings),*
                })
            }
        }

        #[cfg(all(feature = "backend-adapters", not(coverage)))]
        impl ::storeit_core::RowAdapter<#struct_name> for #adapter_struct_name {
            type Row = ::libsql::Row;
            fn from_row(&self, row: &Self::Row) -> ::storeit_core::RepoResult<#struct_name> {
                Ok(#struct_name {
                    #(#libsql_get_mappings),*
                })
            }
        }
    };

    // --- Combine all generated code ---
    let expanded = quote! {
        #fetchable_impl
        #identifiable_impl
        #insertable_impl
        #updatable_impl
        #row_adapter_impls
    };

    TokenStream::from(expanded)
}

// --- `repository` attribute macro ---

/// Struct to parse a finder like `find_by_email: String`
struct Finder {
    name: Ident,
    ty: Type,
}

impl Parse for Finder {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        Ok(Finder { name, ty })
    }
}

/// Struct for parsing the main macro arguments
struct RepositoryArgs {
    entity: Type,
    backend: Ident,
    finders: Option<Punctuated<Finder, Token![,]>>,
}

impl Parse for RepositoryArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut entity = None;
        let mut backend = None;
        let mut finders = None;

        let attrs = Punctuated::<syn::Meta, Token![,]>::parse_terminated(input)?;
        for meta in attrs {
            match meta {
                syn::Meta::NameValue(nv) => {
                    let ident_str = nv
                        .path
                        .get_ident()
                        .ok_or_else(|| syn::Error::new(nv.path.span(), "Expected an identifier"))?
                        .to_string();
                    match ident_str.as_str() {
                        "entity" => {
                            if let syn::Expr::Path(expr_path) = nv.value {
                                entity = Some(Type::Path(TypePath {
                                    qself: None,
                                    path: expr_path.path,
                                }));
                            } else {
                                return Err(syn::Error::new(
                                    nv.value.span(),
                                    "Expected a type for `entity`",
                                ));
                            }
                        }
                        "backend" => {
                            if let syn::Expr::Path(expr_path) = nv.value {
                                backend = expr_path.path.get_ident().cloned();
                            } else {
                                return Err(syn::Error::new(
                                    nv.value.span(),
                                    "Expected an identifier for `backend`",
                                ));
                            }
                        }
                        _ => return Err(syn::Error::new(nv.path.span(), "Unknown attribute")),
                    }
                }
                syn::Meta::List(list) => {
                    if list.path.is_ident("finders") {
                        let parsed_finders = list
                            .parse_args_with(Punctuated::<Finder, Token![,]>::parse_terminated)?;
                        finders = Some(parsed_finders);
                    } else {
                        return Err(syn::Error::new(list.path.span(), "Unknown attribute list"));
                    }
                }
                _ => return Err(syn::Error::new(meta.span(), "Unsupported attribute format")),
            }
        }
        Ok(RepositoryArgs {
            entity: entity
                .ok_or_else(|| syn::Error::new(input.span(), "`entity` is a required attribute"))?,
            backend: backend.ok_or_else(|| {
                syn::Error::new(input.span(), "`backend` is a required attribute")
            })?,
            finders,
        })
    }
}

#[proc_macro_attribute]
pub fn repository(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as RepositoryArgs);
    let input_mod = parse_macro_input!(item as ItemMod);

    let mod_name = &input_mod.ident;
    let entity_ty = &args.entity;

    // Build the path to the generated <Entity>RowAdapter type by replacing the
    // last path segment ident with "<Entity>RowAdapter".
    let adapter_path_ts: proc_macro2::TokenStream = match entity_ty {
        syn::Type::Path(tp) => {
            let mut p = tp.path.clone();
            if let Some(last) = p.segments.last_mut() {
                let adapter_ident =
                    syn::Ident::new(&format!("{}RowAdapter", last.ident), last.ident.span());
                last.ident = adapter_ident;
                last.arguments = syn::PathArguments::None;
            }
            quote! { #p }
        }
        _ => panic!("`entity` must be a type path for repository macro"),
    };

    let (backend_repo_ty, backend_row_ty) = match args.backend.to_string().as_str() {
        "TokioPostgres" => (
            quote! { ::storeit::backends::TokioPostgresRepository },
            quote! { ::tokio_postgres::Row },
        ),
        "MysqlAsync" => (
            quote! { ::storeit::backends::MysqlAsyncRepository },
            quote! { ::mysql_async::Row },
        ),
        "Libsql" => (
            quote! { ::storeit::backends::LibsqlRepository },
            quote! { ::libsql::Row },
        ),
        other => panic!(
            "Unsupported backend: `{}`. Supported backends are: TokioPostgres, MysqlAsync, Libsql",
            other
        ),
    };

    let mut find_by_methods = Vec::new();
    if let Some(finders) = &args.finders {
        for finder in finders {
            let method_name = &finder.name;
            let ty = &finder.ty;
            let finder_str = method_name.to_string();
            let field_name_str = finder_str.strip_prefix("find_by_").unwrap_or(&finder_str);
            let field_name_lit = LitStr::new(field_name_str, method_name.span());

            let ty_string = ty.to_token_stream().to_string();
            let param_conversion = match ty_string.as_str() {
                "String" => quote! { ::storeit_core::ParamValue::String(value.clone()) },
                "i32" => quote! { ::storeit_core::ParamValue::I32(*value) },
                "i64" => quote! { ::storeit_core::ParamValue::I64(*value) },
                "f64" => quote! { ::storeit_core::ParamValue::F64(*value) },
                "bool" => quote! { ::storeit_core::ParamValue::Bool(*value) },
                _ => {
                    let err_msg = format!(
                        "Unsupported finder type: {}. Use String, i32, i64, f64, or bool.",
                        ty_string
                    );
                    quote! { compile_error!(#err_msg) }
                }
            };

            find_by_methods.push(quote! {
                pub async fn #method_name(&self, value: &#ty) -> ::storeit_core::RepoResult<Vec<#entity_ty>> {
                    let param = #param_conversion;
                    self.inner.find_by_field(#field_name_lit, param).await
                }
            });
        }
    }

    let expanded = quote! {
        pub mod #mod_name {
            use super::*;
            use ::storeit_core::{RowAdapter, Repository as _};

            pub struct Repository<A>
            where
                A: RowAdapter<#entity_ty, Row = #backend_row_ty> + Send + Sync + 'static,
            {
                inner: #backend_repo_ty<#entity_ty, A>,
            }

            impl<A> Repository<A>
            where
                A: RowAdapter<#entity_ty, Row = #backend_row_ty> + Send + Sync + 'static,
                #backend_repo_ty<#entity_ty, A>: ::storeit_core::Repository<#entity_ty>,
            {
                /// Construct using an explicit adapter instance.
                pub async fn from_url_with_adapter(conn_str: &str, adapter: A) -> ::storeit_core::RepoResult<Self> {
                    let inner = #backend_repo_ty::from_url(conn_str, <#entity_ty as ::storeit_core::Identifiable>::ID_COLUMN, adapter).await?;
                    Ok(Self { inner })
                }

                pub fn new(backend_repo: #backend_repo_ty<#entity_ty, A>) -> Self {
                    Self { inner: backend_repo }
                }

                #(#find_by_methods)*
            }

            // Convenience constructor when using the default generated RowAdapter for the entity.
            // Only available when backend-specific adapters are enabled and coverage is off.
            #[cfg(all(feature = "backend-adapters", not(coverage)))]
            impl Repository<#adapter_path_ts> {
                pub async fn from_url(conn_str: &str) -> ::storeit_core::RepoResult<Self> {
                    let inner = #backend_repo_ty::from_url(
                        conn_str,
                        <#entity_ty as ::storeit_core::Identifiable>::ID_COLUMN,
                        #adapter_path_ts,
                    ).await?;
                    Ok(Self { inner })
                }
            }

            #[::storeit_core::async_trait]
            impl<A> ::storeit_core::Repository<#entity_ty> for Repository<A>
            where
                A: RowAdapter<#entity_ty, Row = #backend_row_ty> + Send + Sync + 'static,
                #backend_repo_ty<#entity_ty, A>: ::storeit_core::Repository<#entity_ty>,
            {
                async fn find_by_id(&self, id: &<#entity_ty as ::storeit_core::Identifiable>::Key) -> ::storeit_core::RepoResult<Option<#entity_ty>> {
                    self.inner.find_by_id(id).await
                }

                async fn find_by_field(&self, field_name: &str, value: ::storeit_core::ParamValue) -> ::storeit_core::RepoResult<Vec<#entity_ty>> {
                    self.inner.find_by_field(field_name, value).await
                }

                async fn insert(&self, entity: &#entity_ty) -> ::storeit_core::RepoResult<#entity_ty> {
                    self.inner.insert(entity).await
                }

                async fn update(&self, entity: &#entity_ty) -> ::storeit_core::RepoResult<#entity_ty> {
                    self.inner.update(entity).await
                }

                async fn delete_by_id(&self, id: &<#entity_ty as ::storeit_core::Identifiable>::Key) -> ::storeit_core::RepoResult<bool> {
                    self.inner.delete_by_id(id).await
                }
            }
        }
    };

    TokenStream::from(expanded)
}
