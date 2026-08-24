//! Integration test: DDL from a SchemaDiff + CRUD + filters on SQLite.

use api_types::QueryParams;
use core_domain::{ContentTypeKind, FieldType, RelationKind, Uid};
use core_schema::{diff, Attribute, Schema, SchemaInfo};
use dynamic_store::base_columns as base;
use dynamic_store::{ddl, dml};
use indexmap::IndexMap;
use sea_orm::sea_query::ExprTrait;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, DbBackend};
use serde_json::{json, Map as JsonMap};

fn schema(uid: &str, singular: &str, plural: &str, attrs: Vec<(&str, Attribute)>) -> Schema {
    Schema {
        uid: Uid::new(uid),
        kind: ContentTypeKind::CollectionType,
        collection_name: None,
        info: SchemaInfo {
            singular_name: singular.into(),
            plural_name: plural.into(),
            display_name: singular.into(),
            description: None,
            icon: None,
        },
        options: Default::default(),
        plugin_options: None,
        attributes: attrs
            .into_iter()
            .map(|(n, a)| (n.to_string(), a))
            .collect::<IndexMap<_, _>>(),
    metadata: None,
    }
}

fn article_schema() -> Schema {
    let mut title = Attribute::new(FieldType::String);
    title.required = true;
    let mut slug = Attribute::new(FieldType::Uid);
    slug.target_field = Some("title".into());
    let mut views = Attribute::new(FieldType::Integer);
    views.default = Some(json!(0));
    let mut rating = Attribute::new(FieldType::Float);
    rating.required = false;
    let mut state = Attribute::new(FieldType::Enumeration);
    state.enum_values = vec!["hot".into(), "cold".into()];
    let author = Attribute {
        attr_type: FieldType::Relation,
        relation: Some(RelationKind::ManyToOne),
        target: Some(Uid::new("api::author.author")),
        ..Default::default()
    };
    let tags = Attribute {
        attr_type: FieldType::Relation,
        relation: Some(RelationKind::ManyToMany),
        target: Some(Uid::new("api::tag.tag")),
        ..Default::default()
    };
    let cover = Attribute {
        attr_type: FieldType::Media,
        multiple: Some(false),
        ..Default::default()
    };
    let seo = Attribute {
        attr_type: FieldType::Component,
        component: Some(Uid::new("shared.seo")),
        repeatable: Some(false),
        ..Default::default()
    };
    let blocks = Attribute {
        attr_type: FieldType::Dynamiczone,
        components: vec![Uid::new("shared.hero")],
        ..Default::default()
    };
    schema(
        "api::article.article",
        "article",
        "articles",
        vec![
            ("title", title),
            ("slug", slug),
            ("views", views),
            ("rating", rating),
            ("state", state),
            ("author", author),
            ("tags", tags),
            ("cover", cover),
            ("seo", seo),
            ("blocks", blocks),
        ],
    )
}

fn simple_ct(uid: &str, name: &str) -> Schema {
    schema(
        uid,
        name,
        &core_domain::pluralize(name),
        vec![("name", Attribute::new(FieldType::String))],
    )
}

fn seo_component() -> Schema {
    let mut s = schema(
        "shared.seo",
        "seo",
        "seos",
        vec![
            ("meta_title", Attribute::new(FieldType::String)),
            ("meta_description", Attribute::new(FieldType::Text)),
        ],
    );
    s.kind = ContentTypeKind::Component;
    s
}

async fn setup() -> (DatabaseConnection, Vec<Schema>) {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    let backend = DbBackend::Sqlite;
    let schemas = vec![
        article_schema(),
        simple_ct("api::author.author", "author"),
        simple_ct("api::tag.tag", "tag"),
        seo_component(),
    ];
    for s in &schemas {
        let d = diff(None, s);
        let actions = ddl::apply_schema_diff(&db, backend, &d, &schemas)
            .await
            .unwrap();
        assert!(actions.iter().any(|a| a.contains("created table")));
    }
    // Phase 2: auxiliary tables (relation join/media/component link tables and
    // inverse FK columns) now that all host tables exist.
    for s in &schemas {
        ddl::apply_aux(&db, backend, s, &schemas).await.unwrap();
    }
    (db, schemas)
}

#[tokio::test]
async fn ddl_creates_expected_tables() {
    let (db, _) = setup().await;
    let tables: Vec<String> = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
            Vec::new(),
        ))
        .await
        .unwrap()
        .iter()
        .map(|r| r.try_get::<String>("", "name").unwrap())
        .collect();
    for expected in [
        "ct_articles",
        "ct_authors",
        "ct_tags",
        "cmp_shared_seo",
        "ct_articles_tags_links",
        "ct_articles_cover_files_links",
        "ct_articles_components",
    ] {
        assert!(
            tables.contains(&expected.to_string()),
            "missing {expected} in {tables:?}"
        );
    }
}

#[tokio::test]
async fn crud_and_filters() {
    let (db, schemas) = setup().await;
    let backend = DbBackend::Sqlite;
    let article = &schemas[0];
    let table = article.table_name();

    // insert an author
    let author = &schemas[1];
    let author_id = dml::insert(
        &db,
        backend,
        &author.table_name(),
        vec![
            (base::DOCUMENT_ID.into(), "doc-author-1".into()),
            (base::PUBLICATION_STATE.into(), "draft".into()),
            (base::CREATED_AT.into(), chrono::Utc::now().into()),
            (base::UPDATED_AT.into(), chrono::Utc::now().into()),
            ("name".into(), "Ada".into()),
            (base::SYNC_VERSION.into(), 1_i64.into()),
        ],
    )
    .await
    .unwrap();
    assert!(author_id > 0);

    // insert two articles
    for (title, views, state_val) in [("Rust rocks", 10, "hot"), ("Other post", 3, "cold")] {
        let mut data = JsonMap::new();
        data.insert("title".into(), json!(title));
        data.insert("slug".into(), json!(title.to_lowercase().replace(' ', "-")));
        data.insert("views".into(), json!(views));
        data.insert("state".into(), json!(state_val));
        let mut values = dml::build_write_values(article, &data, false).unwrap();
        values.extend([
            (base::DOCUMENT_ID.into(), format!("doc-{views}").into()),
            (base::PUBLICATION_STATE.into(), "draft".into()),
            (base::CREATED_AT.into(), chrono::Utc::now().into()),
            (base::UPDATED_AT.into(), chrono::Utc::now().into()),
            ("author_id".into(), author_id.into()),
            (base::SYNC_VERSION.into(), 1_i64.into()),
        ]);
        dml::insert(&db, backend, &table, values).await.unwrap();
    }

    // select all
    let spec = dml::SelectSpec {
        query: None,
        locale: None,
        state: None,
        default_sort: &[],
    };
    let (rows, total) = dml::select(&db, backend, article, &spec).await.unwrap();
    assert_eq!(total, 2);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["title"], json!("Rust rocks"));
    // The manyToOne relation FK is read back under the schema attribute name
    // (not the physical `author_id` column).
    assert_eq!(rows[0]["author"], json!(author_id));
    assert_eq!(rows[0]["publicationState"], json!("draft"));

    // filter: title contains "rust" (case-insensitive) AND views >= 5
    let q = QueryParams::parse(
        "filters[title][$containsi]=rust&filters[views][$gte]=5&sort[0]=views:desc",
    )
    .unwrap();
    let spec = dml::SelectSpec {
        query: Some(&q),
        locale: None,
        state: None,
        default_sort: &[],
    };
    let (rows, total) = dml::select(&db, backend, article, &spec).await.unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0]["title"], json!("Rust rocks"));

    // filter: $or + $in
    let q = QueryParams::parse("filters[$or][0][views][$lt]=5&filters[$or][1][state][$in][0]=hot")
        .unwrap();
    let spec = dml::SelectSpec {
        query: Some(&q),
        locale: None,
        state: None,
        default_sort: &[],
    };
    let (_, total) = dml::select(&db, backend, article, &spec).await.unwrap();
    assert_eq!(total, 2);

    // find by document id
    let row = dml::find_by_document_id(&db, backend, article, "doc-10", None, None)
        .await
        .unwrap()
        .expect("found");
    assert_eq!(row["slug"], json!("rust-rocks"));

    // update
    dml::update_by_id(
        &db,
        backend,
        &table,
        row["id"].as_i64().unwrap(),
        vec![("views".into(), 42_i64.into())],
    )
    .await
    .unwrap();
    let row = dml::find_by_id(&db, backend, article, row["id"].as_i64().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row["views"], json!(42));

    // m2m links
    let tag = &schemas[2];
    let tag_id = dml::insert(
        &db,
        backend,
        &tag.table_name(),
        vec![
            (base::DOCUMENT_ID.into(), "doc-tag-1".into()),
            (base::PUBLICATION_STATE.into(), "draft".into()),
            (base::CREATED_AT.into(), chrono::Utc::now().into()),
            (base::UPDATED_AT.into(), chrono::Utc::now().into()),
            ("name".into(), "rust".into()),
            (base::SYNC_VERSION.into(), 1_i64.into()),
        ],
    )
    .await
    .unwrap();
    dml::replace_join_links(
        &db,
        backend,
        "ct_articles_tags_links",
        "article_id",
        "tag_id",
        "tags_order",
        row["id"].as_i64().unwrap(),
        &[tag_id],
    )
    .await
    .unwrap();
    let links = dml::fetch_join_links(
        &db,
        backend,
        "ct_articles_tags_links",
        "article_id",
        "tag_id",
        "tags_order",
        row["id"].as_i64().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(links, vec![tag_id]);

    // delete
    let n = dml::delete_where(
        &db,
        backend,
        &table,
        sea_orm::sea_query::Condition::all()
            .add(sea_orm::sea_query::Expr::col(sea_orm::sea_query::Alias::new("views")).lt(5)),
    )
    .await
    .unwrap();
    assert_eq!(n, 1);
    let (_, total) = dml::select(&db, backend, article, &spec_all())
        .await
        .unwrap();
    assert_eq!(total, 1);
}

fn spec_all() -> dml::SelectSpec<'static> {
    dml::SelectSpec {
        query: None,
        locale: None,
        state: None,
        default_sort: &[],
    }
}

#[tokio::test]
async fn alter_adds_column() {
    let (db, schemas) = setup().await;
    let backend = DbBackend::Sqlite;
    let mut v2 = schemas[0].clone();
    v2.attributes
        .insert("subtitle".to_string(), Attribute::new(FieldType::String));
    let d = diff(Some(&schemas[0]), &v2);
    assert_eq!(d.added_attrs.len(), 1);
    ddl::apply_schema_diff(&db, backend, &d, &schemas)
        .await
        .unwrap();

    // subtitle is writable now
    let mut data = JsonMap::new();
    data.insert("title".into(), json!("x"));
    data.insert("subtitle".into(), json!("y"));
    let mut values = dml::build_write_values(&v2, &data, false).unwrap();
    values.extend([
        (base::DOCUMENT_ID.into(), "doc-x".into()),
        (base::PUBLICATION_STATE.into(), "draft".into()),
        (base::CREATED_AT.into(), chrono::Utc::now().into()),
        (base::UPDATED_AT.into(), chrono::Utc::now().into()),
        (base::SYNC_VERSION.into(), 1_i64.into()),
    ]);
    let id = dml::insert(&db, backend, &v2.table_name(), values)
        .await
        .unwrap();
    let row = dml::find_by_id(&db, backend, &v2, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row["subtitle"], json!("y"));
}

#[tokio::test]
async fn insert_one_rejects_invalid_payload() {
    let (db, _schemas) = setup().await;

    // Build a constrained content type: required title, bounded qty, patterned sku.
    let mut title = Attribute::new(FieldType::String);
    title.required = true;
    let mut qty = Attribute::new(FieldType::Integer);
    qty.min = Some(json!(1));
    qty.max = Some(json!(100));
    let mut sku = Attribute::new(FieldType::String);
    sku.regex = Some("^[A-Z]{2}[0-9]{3}$".into());
    let s = schema(
        "api::product.product",
        "product",
        "products",
        vec![
            ("title", title),
            ("qty", qty),
            ("sku", sku),
        ],
    );
    let d = diff(None, &s);
    ddl::apply_schema_diff(&db, DbBackend::Sqlite, &d, &[s.clone()])
        .await
        .unwrap();

    // Missing required field -> validation error.
    let err = dml::insert_one(&db, &s, &json!({"qty": 5}), None)
        .await
        .unwrap_err();
    match err {
        dynamic_store::StoreError::Validation(e) => {
            assert!(e.iter().any(|e| e.field == "title" && e.code == "required"));
        }
        other => panic!("expected Validation error, got {other:?}"),
    }

    // Out-of-range value -> validation error.
    let err = dml::insert_one(
        &db,
        &s,
        &json!({"title": "T", "qty": 0, "sku": "AB123"}),
        None,
    )
    .await
    .unwrap_err();
    match err {
        dynamic_store::StoreError::Validation(e) => {
            assert!(e.iter().any(|e| e.field == "qty" && e.code == "min"));
        }
        other => panic!("expected Validation error, got {other:?}"),
    }

    // Pattern violation -> validation error.
    let err = dml::insert_one(
        &db,
        &s,
        &json!({"title": "T", "qty": 5, "sku": "nope"}),
        None,
    )
    .await
    .unwrap_err();
    match err {
        dynamic_store::StoreError::Validation(e) => {
            assert!(e.iter().any(|e| e.field == "sku" && e.code == "regex"));
        }
        other => panic!("expected Validation error, got {other:?}"),
    }

    // Valid payload -> inserted.
    let row = dml::insert_one(
        &db,
        &s,
        &json!({"title": "T", "qty": 5, "sku": "AB123"}),
        None,
    )
    .await
    .expect("valid payload inserts");
    let doc_id = row["documentId"].as_str().unwrap().to_string();

    // Partial update rejecting a bad value (required not enforced on update).
    let err = dml::update_one(&db, &s, &doc_id, &json!({"qty": 999}), None)
        .await
        .unwrap_err();
    match err {
        dynamic_store::StoreError::Validation(e) => {
            assert!(e.iter().any(|e| e.field == "qty" && e.code == "max"));
        }
        other => panic!("expected Validation error, got {other:?}"),
    }

    // Partial update with only valid provided fields succeeds even though
    // `sku`/`title` required constraints are not part of the payload.
    let updated = dml::update_one(&db, &s, &doc_id, &json!({"qty": 7}), None)
        .await
        .expect("partial update succeeds");
    assert_eq!(updated["qty"], json!(7));
}
