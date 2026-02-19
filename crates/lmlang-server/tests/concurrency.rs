//! Integration tests for multi-agent concurrency endpoints and behavior.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use lmlang_server::concurrency::LockManager;
use lmlang_server::router::build_router;
use lmlang_server::state::AppState;

fn test_app() -> Router {
    let state = AppState::in_memory().expect("failed to create in-memory AppState");
    build_router(state)
}

async fn request_json(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<serde_json::Value>,
    headers: &[(&str, String)],
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(path);
    for (k, v) in headers {
        builder = builder.header(*k, v);
    }

    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap())
        }
        None => Body::empty(),
    };

    let response = app.clone().oneshot(builder.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(json!(null));
    (status, json)
}

async fn post_json(
    app: &Router,
    path: &str,
    body: serde_json::Value,
    headers: &[(&str, String)],
) -> (StatusCode, serde_json::Value) {
    request_json(app, Method::POST, path, Some(body), headers).await
}

async fn get_json(app: &Router, path: &str) -> (StatusCode, serde_json::Value) {
    request_json(app, Method::GET, path, None, &[]).await
}

async fn delete_json(
    app: &Router,
    path: &str,
    headers: &[(&str, String)],
) -> (StatusCode, serde_json::Value) {
    request_json(app, Method::DELETE, path, None, headers).await
}

async fn register_agent(app: &Router, name: &str) -> Uuid {
    let (status, body) = post_json(app, "/agents/register", json!({ "name": name }), &[]).await;
    assert_eq!(status, StatusCode::OK, "register agent failed: {body:?}");
    Uuid::parse_str(body["agent_id"].as_str().unwrap()).unwrap()
}

async fn setup_program(app: &Router, name: &str) -> i64 {
    let (status, body) = post_json(app, "/programs", json!({ "name": name }), &[]).await;
    assert_eq!(status, StatusCode::OK, "create program failed: {body:?}");
    let pid = body["id"].as_i64().unwrap();

    let (status, load_body) = post_json(app, &format!("/programs/{pid}/load"), json!({}), &[]).await;
    assert_eq!(status, StatusCode::OK, "load program failed: {load_body:?}");

    pid
}

async fn add_function(
    app: &Router,
    program_id: i64,
    name: &str,
    headers: &[(&str, String)],
) -> u32 {
    let (status, body) = post_json(
        app,
        &format!("/programs/{program_id}/mutations"),
        json!({
            "mutations": [{
                "type": "AddFunction",
                "name": name,
                "module": 0,
                "params": [],
                "return_type": 3,
                "visibility": "Public"
            }],
            "dry_run": false
        }),
        headers,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "add function failed: {body:?}");
    assert!(body["valid"].as_bool().unwrap(), "add function invalid: {body:?}");
    body["created"][0]["id"].as_u64().unwrap() as u32
}

async fn build_two_function_program(app: &Router) -> (i64, u32, u32) {
    let pid = setup_program(app, "concurrency-test").await;
    let f1 = add_function(app, pid, "f1", &[]).await;
    let f2 = add_function(app, pid, "f2", &[]).await;
    (pid, f1, f2)
}

#[tokio::test]
async fn test_agent_registration_and_listing() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let b = register_agent(&app, "Agent B").await;

    let (status, body) = get_json(&app, "/agents").await;
    assert_eq!(status, StatusCode::OK);
    let listed = body["agents"].as_array().unwrap();
    assert_eq!(listed.len(), 2);

    let (status, _) = delete_json(&app, &format!("/agents/{a}"), &[]).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get_json(&app, "/agents").await;
    assert_eq!(status, StatusCode::OK);
    let listed = body["agents"].as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["agent_id"].as_str().unwrap(), b.to_string());
}

#[tokio::test]
async fn test_concurrent_reads_different_functions() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let b = register_agent(&app, "Agent B").await;
    let (pid, f1, f2) = build_two_function_program(&app).await;

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1], "mode": "read", "description": null }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f2], "mode": "read", "description": null }),
        &[("X-Agent-Id", b.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = get_json(&app, &format!("/programs/{pid}/overview")).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get_json(&app, &format!("/programs/{pid}/functions/{f1}")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/release"),
        json!({ "function_ids": [f1] }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/release"),
        json!({ "function_ids": [f2] }),
        &[("X-Agent-Id", b.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_write_lock_prevents_concurrent_modification() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let b = register_agent(&app, "Agent B").await;
    let (pid, f1, _) = build_two_function_program(&app).await;

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1], "mode": "write", "description": "editing" }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, denied) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1], "mode": "write", "description": null }),
        &[("X-Agent-Id", b.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::LOCKED);
    assert_eq!(denied["error"]["code"], "LOCK_DENIED");

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/release"),
        json!({ "function_ids": [f1] }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1], "mode": "write", "description": null }),
        &[("X-Agent-Id", b.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_batch_lock_acquisition_all_or_nothing() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let b = register_agent(&app, "Agent B").await;
    let pid = setup_program(&app, "batch-test").await;
    let f1 = add_function(&app, pid, "f1", &[]).await;
    let f2 = add_function(&app, pid, "f2", &[]).await;
    let f3 = add_function(&app, pid, "f3", &[]).await;

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f2], "mode": "write", "description": null }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1, f2, f3], "mode": "write", "description": null }),
        &[("X-Agent-Id", b.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::LOCKED);

    let (status, locks) = get_json(&app, &format!("/programs/{pid}/locks")).await;
    assert_eq!(status, StatusCode::OK);
    let lock_entries = locks["locks"].as_array().unwrap();
    assert_eq!(lock_entries.len(), 1, "batch acquire should not partially lock");

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/release"),
        json!({ "function_ids": [f2] }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1, f2, f3], "mode": "write", "description": null }),
        &[("X-Agent-Id", b.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_conflict_detection_on_hash_mismatch() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let pid = setup_program(&app, "conflict-test").await;
    let f1 = add_function(&app, pid, "f1", &[]).await;

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1], "mode": "write", "description": null }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = post_json(
        &app,
        &format!("/programs/{pid}/mutations"),
        json!({
            "mutations": [{
                "type": "InsertNode",
                "op": {"Core": {"Const": {"value": {"I32": 1}}}},
                "owner": f1
            }],
            "dry_run": false,
            "expected_hashes": { (f1.to_string()): "deadbeef" }
        }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "CONFLICT");
    assert!(body["error"]["details"].is_array());
}

#[tokio::test]
async fn test_non_agent_mutations_backward_compatible() {
    let app = test_app();
    let pid = setup_program(&app, "compat-test").await;

    let (status, body) = post_json(
        &app,
        &format!("/programs/{pid}/mutations"),
        json!({
            "mutations": [{
                "type": "AddFunction",
                "name": "plain",
                "module": 0,
                "params": [],
                "return_type": 3,
                "visibility": "Public"
            }],
            "dry_run": false
        }),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["valid"].as_bool().unwrap());
    assert!(body["committed"].as_bool().unwrap());
}

#[tokio::test]
async fn test_verification_failure_rejects_mutation_and_keeps_graph_valid() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let pid = setup_program(&app, "verify-test").await;
    let f1 = add_function(&app, pid, "f1", &[]).await;

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1], "mode": "write", "description": null }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, before) = get_json(&app, &format!("/programs/{pid}/overview")).await;
    assert_eq!(status, StatusCode::OK);

    // BinaryArith without inputs should fail validation and leave graph unchanged.
    let (status, body) = post_json(
        &app,
        &format!("/programs/{pid}/mutations"),
        json!({
            "mutations": [{
                "type": "InsertNode",
                "op": {"Core": {"BinaryArith": {"op": "Add"}}},
                "owner": f1
            }],
            "dry_run": false
        }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body["valid"].as_bool().unwrap());
    assert!(!body["committed"].as_bool().unwrap());

    let (status, after) = get_json(&app, &format!("/programs/{pid}/overview")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before["node_count"], after["node_count"]);
}

#[tokio::test]
async fn test_lock_status_endpoint() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let b = register_agent(&app, "Agent B").await;
    let (pid, f1, f2) = build_two_function_program(&app).await;

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1], "mode": "write", "description": "editing auth" }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f2], "mode": "read", "description": null }),
        &[("X-Agent-Id", b.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get_json(&app, &format!("/programs/{pid}/locks")).await;
    assert_eq!(status, StatusCode::OK);
    let locks = body["locks"].as_array().unwrap();
    assert_eq!(locks.len(), 2);
    assert!(locks.iter().any(|l| l["holder_description"] == "editing auth"));
}

#[tokio::test]
async fn test_global_write_lock_for_structure_changes() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let pid = setup_program(&app, "structure-test").await;

    // Agent can submit AddFunction without a per-function lock.
    let (status, body) = post_json(
        &app,
        &format!("/programs/{pid}/mutations"),
        json!({
            "mutations": [{
                "type": "AddFunction",
                "name": "new_fn",
                "module": 0,
                "params": [],
                "return_type": 3,
                "visibility": "Public"
            }],
            "dry_run": false
        }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["valid"].as_bool().unwrap());
    assert!(body["committed"].as_bool().unwrap());

    let (status, overview) = get_json(&app, &format!("/programs/{pid}/overview")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(overview["functions"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_deregister_releases_all_locks() {
    let app = test_app();
    let a = register_agent(&app, "Agent A").await;
    let (pid, f1, f2) = build_two_function_program(&app).await;

    let (status, _) = post_json(
        &app,
        &format!("/programs/{pid}/locks/acquire"),
        json!({ "function_ids": [f1, f2], "mode": "write", "description": null }),
        &[("X-Agent-Id", a.to_string())],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = delete_json(&app, &format!("/agents/{a}"), &[]).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = get_json(&app, &format!("/programs/{pid}/locks")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locks"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_lock_ttl_expiry_releases_abandoned_locks() {
    let manager = LockManager::new(Duration::from_millis(1));
    let agent = lmlang_server::concurrency::AgentId(Uuid::new_v4());

    manager
        .try_acquire_write(&agent, lmlang_core::id::FunctionId(1), None)
        .expect("lock acquire should succeed");

    std::thread::sleep(Duration::from_millis(5));
    let released = manager.sweep_expired_locks();
    assert_eq!(released, vec![lmlang_core::id::FunctionId(1)]);
}
