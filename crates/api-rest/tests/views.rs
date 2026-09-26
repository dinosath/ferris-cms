use api_rest::{build_router, AppState};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use db::{connect_sqlite_memory, seed, Migrator};
use sea_orm_migration::MigratorTrait;
use services::{load_schema_cache, AppConfig};
use std::sync::Arc;
use tower::ServiceExt;

async fn setup() -> axum::Router {
    let db = connect_sqlite_memory().await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    seed::seed(&db).await.unwrap();
    let state = Arc::new(AppState::new(
        db.clone(),
        AppConfig {
            db_driver: "sqlite".into(),
            jwt_secret: "views-test".into(),
            jwt_expiry_secs: 3600,
            admin_registration_open: true,
            media_storage_dir: std::env::temp_dir().display().to_string(),
        },
    ));
    load_schema_cache(&db, &state.ctx.schema_cache)
        .await
        .unwrap();
    let _ = state.ctx.init_rbac().await;
    build_router(state)
}

fn request(method: &str, uri: &str, body: serde_json::Value, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

async fn json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn admin(router: &axum::Router) -> String {
    let response = router
        .clone()
        .oneshot(request(
            "POST",
            "/admin/register-admin",
            serde_json::json!({"email":"views@test.dev","password":"StrongPass123!"}),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json(response).await["data"]["token"]
        .as_str()
        .unwrap()
        .to_string()
}

async fn schema(router: &axum::Router, token: &str, uid: &str) {
    let response = router.clone().oneshot(request("POST", "/content-type-builder/schema", serde_json::json!({"schemas":[{
        "uid":uid,"kind":"collectionType","info":{"singularName":"product","pluralName":"products","displayName":"Product"},
        "options":{"draftAndPublish":false},"attributes":{"name":{"type":"string"},"active":{"type":"boolean"},"status":{"type":"enumeration","enum":["draft","active","archived"]}}
    }]}), Some(token))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn views_crud_relationship_query_and_lifecycle() {
    let router = setup().await;
    let token = admin(&router).await;
    let uid = "api::product.product";
    schema(&router, &token, uid).await;
    schema(&router, &token, "api::other.other").await;

    let listed = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/admin/content-manager/content-types/{uid}/views"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let all = json(listed).await["data"].as_array().unwrap().clone();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["name"], "All Records");
    let default_id = all[0]["id"].as_i64().unwrap();

    let created = router.clone().oneshot(request("POST", &format!("/admin/content-manager/content-types/{uid}/views"), serde_json::json!({
        "name":"Active Products","viewType":"grid","configuration":{
            "filters":{"leaf":{"field":"active","op":"$eq","values":[true]}},
            "sorts":[{"field":"name","descending":false}],"columns":[{"fieldId":"name","visible":true,"position":0}],"pageSize":25
        }
    }), Some(&token))).await.unwrap();
    assert_eq!(created.status(), StatusCode::OK);
    let active = json(created).await["data"].clone();
    let active_id = active["id"].as_i64().unwrap();
    assert!(!active["isDefault"].as_bool().unwrap());

    let record = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            serde_json::json!({"data":{"name":"IPA","active":true,"status":"active"}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(record.status(), StatusCode::OK);
    let record = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("/admin/content-manager/collection-types/{uid}"),
            serde_json::json!({"data":{"name":"Archived","active":false,"status":"archived"}}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(record.status(), StatusCode::OK);

    let queried = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/admin/content-manager/content-types/{uid}/views/{active_id}/records"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(queried.status(), StatusCode::OK);
    let rows = json(queried).await["data"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "IPA");

    let duplicate = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("/admin/content-manager/content-types/{uid}/views/{active_id}/duplicate"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);
    let duplicate_id = json(duplicate).await["data"]["id"].as_i64().unwrap();
    assert_ne!(duplicate_id, active_id);

    let set_default = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("/admin/content-manager/content-types/{uid}/views/{active_id}/default"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(set_default.status(), StatusCode::OK);
    let after_default = json(set_default).await["data"]["isDefault"]
        .as_bool()
        .unwrap();
    assert!(after_default);

    let reordered = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("/admin/content-manager/content-types/{uid}/views/reorder"),
            serde_json::json!({"ids":[active_id,default_id,duplicate_id]}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(reordered.status(), StatusCode::OK);
    assert_eq!(json(reordered).await["data"][0]["id"], active_id);

    let wrong_type = router
        .clone()
        .oneshot(request(
            "GET",
            "/admin/content-manager/content-types/api::other.other/views/1",
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(wrong_type.status(), StatusCode::NOT_FOUND);

    let deleted = router
        .clone()
        .oneshot(request(
            "DELETE",
            &format!("/admin/content-manager/content-types/{uid}/views/{active_id}"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);
    let remaining = router
        .clone()
        .oneshot(request(
            "GET",
            &format!("/admin/content-manager/content-types/{uid}/views"),
            serde_json::json!({}),
            Some(&token),
        ))
        .await
        .unwrap();
    let remaining = json(remaining).await["data"].as_array().unwrap().clone();
    assert!(remaining.iter().any(|v| v["isDefault"] == true));
}
