//! End-to-end integration tests for the lmlang HTTP API.
//!
//! Tests exercise the full stack: HTTP request -> axum router -> handler ->
//! ProgramService -> graph/storage/checker/interpreter -> HTTP response.
//!
//! Each test creates a fresh AppState backed by a unique temp SQLite database.
//! Tests use `tower::ServiceExt::oneshot` to send requests directly to the
//! router without starting a network server.
//!
//! **Note on validation:** Single mutations trigger full-graph validation after
//! each commit. Operations like BinaryArith(Add) that require inputs will fail
//! validation if committed alone. Tests that build multi-node graphs use batch
//! mutations to add all nodes and edges atomically.

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use tower::ServiceExt;

use lmlang_server::router::build_router;
use lmlang_server::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Creates a fresh router backed by a unique temp database.
fn test_app() -> Router {
    let state = AppState::in_memory().expect("failed to create in-memory AppState");
    build_router(state)
}

/// Creates a router backed by an on-disk SQLite database path.
fn test_app_with_db(db_path: &str) -> Router {
    let state = AppState::new(db_path).expect("failed to create AppState");
    build_router(state)
}

fn temp_db_path(prefix: &str) -> String {
    std::env::temp_dir()
        .join(format!("{}_{}.db", prefix, uuid::Uuid::new_v4()))
        .to_string_lossy()
        .to_string()
}

#[derive(Clone)]
struct MockPlannerState {
    response_content: String,
    requests: std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
}

async fn mock_planner_chat(
    State(state): State<MockPlannerState>,
    Json(request): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    state.requests.lock().unwrap().push(request);
    Json(json!({
        "choices": [{
            "message": {
                "content": state.response_content
            }
        }]
    }))
}

async fn start_mock_planner_server(
    response_content: &str,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    tokio::task::JoinHandle<()>,
) {
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let state = MockPlannerState {
        response_content: response_content.to_string(),
        requests: requests.clone(),
    };

    let app = Router::new()
        .route("/chat/completions", post(mock_planner_chat))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock planner server");
    let addr = listener.local_addr().expect("failed to read mock server addr");
    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{}", addr), requests, handle)
}

/// Sends a POST request with a JSON body and returns (status, json).
async fn post_json(
    app: &Router,
    path: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or(json!(null));
    (status, json)
}

/// Sends a GET request and returns (status, json).
async fn get_json(app: &Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or(json!(null));
    (status, json)
}

/// Sends a GET request and returns (status, body text).
async fn get_text(app: &Router, path: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body_bytes.to_vec()).unwrap_or_default();
    (status, text)
}

/// Creates a program, loads it, returns the program_id.
async fn setup_program(app: &Router) -> i64 {
    let (status, body) = post_json(app, "/programs", json!({ "name": "test_prog" })).await;
    assert_eq!(status, StatusCode::OK, "create program failed: {:?}", body);
    let program_id = body["id"].as_i64().unwrap();

    let (status, _) = post_json(app, &format!("/programs/{}/load", program_id), json!({})).await;
    assert_eq!(status, StatusCode::OK, "load program failed");

    program_id
}

/// Adds a function via single mutation (functions pass validation alone).
async fn add_function(app: &Router, program_id: i64, name: &str) -> u32 {
    let (status, body) = post_json(
        app,
        &format!("/programs/{}/mutations", program_id),
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
    )
    .await;
    assert_eq!(status, StatusCode::OK, "add function failed: {:?}", body);
    assert!(
        body["valid"].as_bool().unwrap(),
        "add function validation failed: {:?}",
        body
    );
    body["created"][0]["id"].as_u64().unwrap() as u32
}

/// Adds a function with typed params via single mutation.
async fn add_typed_function(
    app: &Router,
    program_id: i64,
    name: &str,
    params: serde_json::Value,
    return_type: u32,
) -> u32 {
    let (status, body) = post_json(
        app,
        &format!("/programs/{}/mutations", program_id),
        json!({
            "mutations": [{
                "type": "AddFunction",
                "name": name,
                "module": 0,
                "params": params,
                "return_type": return_type,
                "visibility": "Public"
            }],
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "add typed function failed: {:?}",
        body
    );
    assert!(
        body["valid"].as_bool().unwrap(),
        "add typed function validation failed: {:?}",
        body
    );
    body["created"][0]["id"].as_u64().unwrap() as u32
}

/// Inserts a Const node (passes validation alone since Const has no required inputs).
async fn insert_const(app: &Router, program_id: i64, owner: u32, value: serde_json::Value) -> u32 {
    let (status, body) = post_json(
        app,
        &format!("/programs/{}/mutations", program_id),
        json!({
            "mutations": [{
                "type": "InsertNode",
                "op": {"Core": {"Const": {"value": value}}},
                "owner": owner
            }],
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "insert const failed: {:?}", body);
    assert!(
        body["valid"].as_bool().unwrap(),
        "insert const validation failed: {:?}",
        body
    );
    body["created"][0]["id"].as_u64().unwrap() as u32
}

/// Inserts a Parameter node (passes validation alone since Parameters have no required inputs).
async fn insert_param(app: &Router, program_id: i64, owner: u32, index: u32) -> u32 {
    let (status, body) = post_json(
        app,
        &format!("/programs/{}/mutations", program_id),
        json!({
            "mutations": [{
                "type": "InsertNode",
                "op": {"Core": {"Parameter": {"index": index}}},
                "owner": owner
            }],
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "insert param failed: {:?}", body);
    assert!(
        body["valid"].as_bool().unwrap(),
        "insert param validation failed: {:?}",
        body
    );
    body["created"][0]["id"].as_u64().unwrap() as u32
}

/// Submits a batch mutation and returns the response body.
/// Used to add nodes that require inputs (e.g., BinaryArith) together with their edges.
async fn batch_mutate(
    app: &Router,
    program_id: i64,
    mutations: serde_json::Value,
) -> serde_json::Value {
    let (status, body) = post_json(
        app,
        &format!("/programs/{}/mutations", program_id),
        json!({
            "mutations": mutations,
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "batch mutate failed: {:?}", body);
    body
}

// ===========================================================================
// TOOL-01: propose_structured_edit
// ===========================================================================

/// Test 1: Batch mutation builds a complete function with nodes and edges.
/// Verifies valid=true, committed=true, created entities, and overview.
#[tokio::test]
async fn tool01_single_mutation_workflow() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Add function first (passes validation alone)
    let func_id = add_typed_function(&app, pid, "add", json!([["a", 3], ["b", 3]]), 3).await;

    // Add params (pass validation alone)
    let param_a = insert_param(&app, pid, func_id, 0).await;
    let param_b = insert_param(&app, pid, func_id, 1).await;

    // Add BinaryArith(Add) + edges in a single batch (BinaryArith needs inputs)
    let body = batch_mutate(
        &app,
        pid,
        json!([
            {
                "type": "InsertNode",
                "op": {"Core": {"BinaryArith": {"op": "Add"}}},
                "owner": func_id
            },
            {
                "type": "AddEdge",
                "from": param_a, "to": param_a + 2,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": param_b, "to": param_a + 2,
                "source_port": 0, "target_port": 1,
                "value_type": 3
            }
        ]),
    )
    .await;

    assert!(
        body["valid"].as_bool().unwrap(),
        "batch should be valid: {:?}",
        body
    );
    assert!(body["committed"].as_bool().unwrap(), "batch should commit");
    assert!(
        !body["created"].as_array().unwrap().is_empty(),
        "should have created entities"
    );

    // GET overview and verify
    let (status, overview) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    assert_eq!(status, StatusCode::OK);
    // 3 nodes: param_a, param_b, add_node
    assert_eq!(overview["node_count"].as_u64().unwrap(), 3);
    assert!(overview["edge_count"].as_u64().unwrap() >= 2);
    let functions = overview["functions"].as_array().unwrap();
    assert!(
        !functions.is_empty(),
        "expected at least one function in overview"
    );
}

/// Test 2: dry_run previews without committing.
#[tokio::test]
async fn tool01_dry_run_no_commit() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "test_fn").await;

    let (_, overview_before) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_before = overview_before["node_count"].as_u64().unwrap();

    // dry_run=true: insert a Const node
    let (status, body) = post_json(
        &app,
        &format!("/programs/{}/mutations", pid),
        json!({
            "mutations": [{
                "type": "InsertNode",
                "op": {"Core": {"Const": {"value": {"I32": 42}}}},
                "owner": func_id
            }],
            "dry_run": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["valid"].as_bool().unwrap(),
        "dry_run should report valid=true"
    );
    assert!(
        !body["committed"].as_bool().unwrap(),
        "dry_run should NOT commit"
    );

    let (_, overview_after) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_after = overview_after["node_count"].as_u64().unwrap();
    assert_eq!(
        count_before, count_after,
        "dry_run should not change node count"
    );
}

/// Test 3: Batch mutation atomicity -- failed batch leaves graph unchanged.
#[tokio::test]
async fn tool01_batch_atomicity() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "test_fn").await;

    // Insert a Const node so we have something to measure
    insert_const(&app, pid, func_id, json!({"I32": 1})).await;

    let (_, overview_before) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_before = overview_before["node_count"].as_u64().unwrap();

    // Batch: valid node + invalid edge to nonexistent node
    let (status, body) = post_json(
        &app,
        &format!("/programs/{}/mutations", pid),
        json!({
            "mutations": [
                {
                    "type": "InsertNode",
                    "op": {"Core": {"Const": {"value": {"I32": 99}}}},
                    "owner": func_id
                },
                {
                    "type": "AddEdge",
                    "from": 9999, "to": 9998,
                    "source_port": 0, "target_port": 0,
                    "value_type": 3
                }
            ],
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body["valid"].as_bool().unwrap(),
        "batch with invalid mutation should be invalid"
    );
    assert!(
        !body["committed"].as_bool().unwrap(),
        "batch should NOT commit on failure"
    );
    assert!(
        !body["errors"].as_array().unwrap().is_empty(),
        "should have errors"
    );

    // Verify graph unchanged (first mutation was NOT applied)
    let (_, overview_after) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_after = overview_after["node_count"].as_u64().unwrap();
    assert_eq!(
        count_before, count_after,
        "failed batch must not change graph"
    );
}

// ===========================================================================
// TOOL-02: retrieve_subgraph
// ===========================================================================

/// Test 4: Retrieve a single node by ID with full detail.
#[tokio::test]
async fn tool02_get_node_by_id() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "test_fn").await;

    // Insert a Const node and a second Const, then connect them via an edge
    // (Const -> Const edge is type-valid as long as types match)
    let node_a = insert_const(&app, pid, func_id, json!({"I32": 42})).await;
    let node_b = insert_const(&app, pid, func_id, json!({"I32": 99})).await;

    // Add a data edge between them (Const->Const is allowed in the graph even if unusual)
    let (status, _) = post_json(
        &app,
        &format!("/programs/{}/mutations", pid),
        json!({
            "mutations": [{
                "type": "AddEdge",
                "from": node_a, "to": node_b,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            }],
            "dry_run": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // GET node with full detail
    let (status, body) = get_json(
        &app,
        &format!("/programs/{}/nodes/{}?detail=Full", pid, node_b),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"].as_u64().unwrap() as u32, node_b);
    // Full detail should include incoming_edges and outgoing_edges
    assert!(
        body["incoming_edges"].is_array(),
        "full detail should include incoming_edges"
    );
    assert!(
        body["outgoing_edges"].is_array(),
        "full detail should include outgoing_edges"
    );
    // node_b should have 1 incoming edge from node_a
    assert!(
        !body["incoming_edges"].as_array().unwrap().is_empty(),
        "should have incoming edge"
    );
}

/// Test 5: Retrieve function boundary -- only nodes/edges from that function.
#[tokio::test]
async fn tool02_get_function_boundary() {
    let app = test_app();
    let pid = setup_program(&app).await;

    let func1 = add_function(&app, pid, "func1").await;
    let func2 = add_function(&app, pid, "func2").await;

    // Add Const nodes (pass validation alone)
    insert_const(&app, pid, func1, json!({"I32": 1})).await;
    insert_const(&app, pid, func1, json!({"I32": 2})).await;
    insert_const(&app, pid, func2, json!({"I32": 3})).await;

    // GET func1 -- should have 2 nodes
    let (status, body) = get_json(
        &app,
        &format!("/programs/{}/functions/{}?detail=Standard", pid, func1),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["function"]["name"].as_str().unwrap(), "func1");
    assert_eq!(body["nodes"].as_array().unwrap().len(), 2);

    // GET func2 -- should have 1 node
    let (status, body) = get_json(
        &app,
        &format!("/programs/{}/functions/{}?detail=Standard", pid, func2),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["function"]["name"].as_str().unwrap(), "func2");
    assert_eq!(body["nodes"].as_array().unwrap().len(), 1);
}

/// Test 6: N-hop neighborhood -- A -> B -> C -> D, 2 hops from A should include A, B, C but not D.
#[tokio::test]
async fn tool02_neighborhood_n_hop() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "chain_fn").await;

    // Create chain of Const nodes: A -> B -> C -> D
    let a = insert_const(&app, pid, func_id, json!({"I32": 1})).await;
    let b = insert_const(&app, pid, func_id, json!({"I32": 2})).await;
    let c = insert_const(&app, pid, func_id, json!({"I32": 3})).await;
    let d = insert_const(&app, pid, func_id, json!({"I32": 4})).await;

    // Add edges (Const->Const allowed in graph structure)
    let edges = json!([
        {"type": "AddEdge", "from": a, "to": b, "source_port": 0, "target_port": 0, "value_type": 3},
        {"type": "AddEdge", "from": b, "to": c, "source_port": 0, "target_port": 0, "value_type": 3},
        {"type": "AddEdge", "from": c, "to": d, "source_port": 0, "target_port": 0, "value_type": 3}
    ]);
    let body = batch_mutate(&app, pid, edges).await;
    assert!(
        body["valid"].as_bool().unwrap(),
        "chain edges should be valid: {:?}",
        body
    );

    // 2-hop neighborhood from A
    let (status, body) = post_json(
        &app,
        &format!("/programs/{}/neighborhood", pid),
        json!({
            "node_id": a,
            "max_hops": 2,
            "detail": "Summary"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let nodes = body["nodes"].as_array().unwrap();
    let node_ids: Vec<u64> = nodes.iter().map(|n| n["id"].as_u64().unwrap()).collect();

    assert!(
        node_ids.contains(&(a as u64)),
        "A should be in 2-hop neighborhood"
    );
    assert!(
        node_ids.contains(&(b as u64)),
        "B should be in 2-hop neighborhood"
    );
    assert!(
        node_ids.contains(&(c as u64)),
        "C should be in 2-hop neighborhood"
    );
    assert!(
        !node_ids.contains(&(d as u64)),
        "D should NOT be in 2-hop neighborhood from A"
    );
}

// ===========================================================================
// TOOL-03: verify_and_propagate
// ===========================================================================

/// Test 7: Type verification catches type mismatch (i32 + f64).
/// Uses batch mutation to build the mismatched graph, then verifies.
#[tokio::test]
async fn tool03_type_mismatch_detected() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Create function with (a: i32, b: f64)
    let func_id = add_typed_function(&app, pid, "bad_add", json!([["a", 3], ["b", 6]]), 3).await;

    // Add params (pass validation alone)
    let param_a = insert_param(&app, pid, func_id, 0).await;
    let param_b = insert_param(&app, pid, func_id, 1).await;

    // Batch: Add BinaryArith(Add) + edges with type mismatch (i32 and f64)
    let body = batch_mutate(
        &app,
        pid,
        json!([
            {
                "type": "InsertNode",
                "op": {"Core": {"BinaryArith": {"op": "Add"}}},
                "owner": func_id
            },
            {
                "type": "AddEdge",
                "from": param_a, "to": param_a + 2,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": param_b, "to": param_a + 2,
                "source_port": 0, "target_port": 1,
                "value_type": 6
            }
        ]),
    )
    .await;

    // The batch may or may not commit depending on whether the type checker
    // catches this at mutation time. Either way, verify catches it:
    let (status, verify_body) = post_json(
        &app,
        &format!("/programs/{}/verify", pid),
        json!({ "scope": "full" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // If batch was rejected (valid=false), verify won't find errors in the graph
    // because the mutation was rolled back. If batch committed, verify finds errors.
    if body["committed"].as_bool().unwrap_or(false) {
        // Graph has the mismatch -- verify should catch it
        assert!(
            !verify_body["valid"].as_bool().unwrap(),
            "should detect type mismatch"
        );

        let errors = verify_body["errors"].as_array().unwrap();
        assert!(!errors.is_empty(), "should have at least one error");

        let error = &errors[0];
        assert_eq!(error["code"].as_str().unwrap(), "TYPE_MISMATCH");
        assert!(
            error["details"].is_object(),
            "should have structured details"
        );
        let details = &error["details"];
        assert!(
            details["source_node"].is_number(),
            "should have source_node"
        );
        assert!(
            details["target_node"].is_number(),
            "should have target_node"
        );
        assert!(
            details["expected_type"].is_number(),
            "should have expected_type"
        );
        assert!(
            details["actual_type"].is_number(),
            "should have actual_type"
        );
    } else {
        // Batch was rejected at mutation time -- check batch errors
        assert!(
            !body["valid"].as_bool().unwrap(),
            "batch with mismatch should be invalid"
        );
        let errors = body["errors"].as_array().unwrap();
        assert!(
            !errors.is_empty(),
            "should have errors from batch rejection"
        );
        // Verify the error is a type mismatch
        let has_type_error = errors
            .iter()
            .any(|e| e["code"].as_str().unwrap_or("") == "TYPE_MISMATCH");
        assert!(
            has_type_error,
            "errors should include TYPE_MISMATCH: {:?}",
            errors
        );

        let error = errors
            .iter()
            .find(|e| e["code"].as_str().unwrap_or("") == "TYPE_MISMATCH")
            .unwrap();
        assert!(
            error["details"].is_object(),
            "should have structured details"
        );
        let details = &error["details"];
        assert!(
            details["source_node"].is_number(),
            "should have source_node"
        );
        assert!(
            details["target_node"].is_number(),
            "should have target_node"
        );
    }
}

// ===========================================================================
// TOOL-04: simulate function execution
// ===========================================================================

/// Test 8: Simulate add(3, 5) = 8 with trace.
/// Builds the complete function with a batch mutation.
#[tokio::test]
async fn tool04_simulate_add_function() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Create add(a: i32, b: i32) -> i32
    let func_id = add_typed_function(&app, pid, "add", json!([["a", 3], ["b", 3]]), 3).await;

    // Add params (pass validation alone)
    let param_a = insert_param(&app, pid, func_id, 0).await;
    let param_b = insert_param(&app, pid, func_id, 1).await;

    // Batch: add BinaryArith(Add) + Return + all edges
    let add_node_id = param_b + 1; // next available node id
    let ret_node_id = param_b + 2;

    let body = batch_mutate(
        &app,
        pid,
        json!([
            {
                "type": "InsertNode",
                "op": {"Core": {"BinaryArith": {"op": "Add"}}},
                "owner": func_id
            },
            {
                "type": "InsertNode",
                "op": {"Core": "Return"},
                "owner": func_id
            },
            {
                "type": "AddEdge",
                "from": param_a, "to": add_node_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": param_b, "to": add_node_id,
                "source_port": 0, "target_port": 1,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": add_node_id, "to": ret_node_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            }
        ]),
    )
    .await;
    assert!(
        body["valid"].as_bool().unwrap(),
        "add function batch should be valid: {:?}",
        body
    );
    assert!(
        body["committed"].as_bool().unwrap(),
        "should commit: {:?}",
        body
    );

    // Simulate with inputs [3, 5]
    let (status, body) = post_json(
        &app,
        &format!("/programs/{}/simulate", pid),
        json!({
            "function_id": func_id,
            "inputs": [3, 5],
            "trace_enabled": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["success"].as_bool().unwrap(),
        "simulation should succeed: {:?}",
        body
    );

    // Check result = 8 (Value::I32(8) serializes as {"I32": 8})
    let result = &body["result"];
    assert_eq!(result["I32"].as_i64().unwrap(), 8, "3 + 5 should equal 8");

    // Check trace is present when trace_enabled=true
    assert!(
        body["trace"].is_array(),
        "trace should be present when enabled"
    );
    let trace = body["trace"].as_array().unwrap();
    assert!(!trace.is_empty(), "trace should have entries");
}

// ===========================================================================
// TOOL-05: HTTP/JSON endpoints accessible
// ===========================================================================

/// Test 9: Verify Content-Type and error handling.
#[tokio::test]
async fn tool05_http_json_format() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Verify GET returns JSON Content-Type
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&format!("/programs/{}/overview", pid))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("application/json"),
        "Content-Type should be application/json, got: {}",
        content_type
    );

    // Verify invalid JSON body returns structured error
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&format!("/programs/{}/mutations", pid))
                .header("content-type", "application/json")
                .body(Body::from("this is not json"))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "invalid JSON should return 400 or 422, got: {}",
        status
    );
}

// ===========================================================================
// TOOL-06: structured diagnostics with graph context
// ===========================================================================

/// Test 10: Structured diagnostics include error codes and node IDs, no fix suggestions.
#[tokio::test]
async fn tool06_structured_diagnostics() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Create function with (x: i32, y: f64)
    let func_id = add_typed_function(&app, pid, "mistyped", json!([["x", 3], ["y", 6]]), 3).await;

    // Add params
    let param_x = insert_param(&app, pid, func_id, 0).await;
    let param_y = insert_param(&app, pid, func_id, 1).await;

    // Batch add BinaryArith + mismatched edges
    let add_node_id = param_y + 1;
    let body = batch_mutate(
        &app,
        pid,
        json!([
            {
                "type": "InsertNode",
                "op": {"Core": {"BinaryArith": {"op": "Add"}}},
                "owner": func_id
            },
            {
                "type": "AddEdge",
                "from": param_x, "to": add_node_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": param_y, "to": add_node_id,
                "source_port": 0, "target_port": 1,
                "value_type": 6
            }
        ]),
    )
    .await;

    // Get the errors either from batch rejection or from verify
    let errors = if !body["valid"].as_bool().unwrap_or(true) {
        body["errors"].as_array().unwrap().clone()
    } else {
        let (status, verify_body) = post_json(
            &app,
            &format!("/programs/{}/verify", pid),
            json!({ "scope": "full" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        verify_body["errors"].as_array().unwrap().clone()
    };

    assert!(!errors.is_empty(), "should have type errors");

    // Find the TYPE_MISMATCH error
    let error = errors
        .iter()
        .find(|e| e["code"].as_str().unwrap_or("") == "TYPE_MISMATCH")
        .expect("should have TYPE_MISMATCH error");

    // Verify structured diagnostic format
    assert!(error["code"].is_string(), "error should have a code field");
    assert!(
        error["message"].is_string(),
        "error should have a message field"
    );

    // Per locked decision: no fix suggestions in diagnostics
    assert!(
        error.get("suggestion").is_none(),
        "should NOT include fix suggestions"
    );
    assert!(error.get("fix").is_none(), "should NOT include fix field");

    // Details should include node IDs and type info
    let details = &error["details"];
    assert!(details.is_object(), "should have structured details");
}

// ===========================================================================
// STORE-03: undo, checkpoint, restore, history
// ===========================================================================

/// Test 11: Undo reverses the last mutation.
#[tokio::test]
async fn store03_undo_reverses_mutation() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "test_fn").await;

    let (_, overview_before) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_before = overview_before["node_count"].as_u64().unwrap();

    // Add a Const node
    insert_const(&app, pid, func_id, json!({"I32": 42})).await;

    let (_, overview_mid) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    assert_eq!(
        overview_mid["node_count"].as_u64().unwrap(),
        count_before + 1
    );

    // Undo
    let (status, body) = post_json(&app, &format!("/programs/{}/undo", pid), json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["success"].as_bool().unwrap(), "undo should succeed");

    let (_, overview_after) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    assert_eq!(
        overview_after["node_count"].as_u64().unwrap(),
        count_before,
        "undo should restore node count"
    );
}

/// Test 12: Named checkpoint and restore.
#[tokio::test]
async fn store03_checkpoint_and_restore() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "test_fn").await;

    // Add a node
    insert_const(&app, pid, func_id, json!({"I32": 1})).await;

    let (_, overview_checkpoint) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_at_checkpoint = overview_checkpoint["node_count"].as_u64().unwrap();

    // Create checkpoint
    let (status, body) = post_json(
        &app,
        &format!("/programs/{}/checkpoints", pid),
        json!({ "name": "before_changes", "description": "snapshot" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"].as_str().unwrap(), "before_changes");

    // Add more nodes
    insert_const(&app, pid, func_id, json!({"I32": 2})).await;
    insert_const(&app, pid, func_id, json!({"I32": 3})).await;

    let (_, overview_after) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    assert!(overview_after["node_count"].as_u64().unwrap() > count_at_checkpoint);

    // Restore checkpoint
    let (status, body) = post_json(
        &app,
        &format!("/programs/{}/checkpoints/before_changes/restore", pid),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["success"].as_bool().unwrap(), "restore should succeed");
    assert_eq!(body["name"].as_str().unwrap(), "before_changes");

    let (_, overview_restored) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    assert_eq!(
        overview_restored["node_count"].as_u64().unwrap(),
        count_at_checkpoint,
        "restore should return to checkpoint state"
    );
}

/// Test 13: List history and checkpoints.
#[tokio::test]
async fn store03_list_history_and_checkpoints() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "test_fn").await;

    // Perform mutations
    insert_const(&app, pid, func_id, json!({"I32": 10})).await;
    insert_const(&app, pid, func_id, json!({"I32": 20})).await;

    // Create checkpoint
    let (status, _) = post_json(
        &app,
        &format!("/programs/{}/checkpoints", pid),
        json!({ "name": "mid_point", "description": "after two inserts" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // GET history
    let (status, history) = get_json(&app, &format!("/programs/{}/history", pid)).await;
    assert_eq!(status, StatusCode::OK);
    let entries = history["entries"].as_array().unwrap();
    // add_function + 2 insert_const = 3 edits minimum
    assert!(
        entries.len() >= 3,
        "expected at least 3 history entries, got {}",
        entries.len()
    );

    for entry in entries {
        assert!(entry["id"].is_string(), "history entry should have id");
        assert!(
            entry["timestamp"].is_string(),
            "history entry should have timestamp"
        );
    }

    // GET checkpoints
    let (status, checkpoints) = get_json(&app, &format!("/programs/{}/checkpoints", pid)).await;
    assert_eq!(status, StatusCode::OK);
    let cp_list = checkpoints["checkpoints"].as_array().unwrap();
    assert!(!cp_list.is_empty(), "should have at least one checkpoint");

    let checkpoint = &cp_list[0];
    assert_eq!(checkpoint["name"].as_str().unwrap(), "mid_point");
    assert!(
        checkpoint["timestamp"].is_string(),
        "checkpoint should have timestamp"
    );
    assert!(
        checkpoint["edit_position"].is_number(),
        "checkpoint should have edit_position"
    );
}

// ===========================================================================
// CNTR-05: Property-based contract testing
// ===========================================================================

/// Test 14: Property test on function with precondition finds violations.
///
/// Builds checked_fn(a: i32) -> i32 with precondition (a >= 0), where the
/// function body returns a. Random i32 inputs will include negatives, causing
/// precondition violations that the property test harness should detect.
#[tokio::test]
async fn cntr05_property_test_finds_violations() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Create checked_fn(a: i32) -> i32
    let func_id = add_typed_function(&app, pid, "checked_fn", json!([["a", 3]]), 3).await;

    // Add parameter node
    let param_a = insert_param(&app, pid, func_id, 0).await;

    // Build: Const(0), Compare(Ge, a, 0), Precondition, Return
    // Precondition -> Return (control edge ensures contract checked before return)
    let const_zero_id = param_a + 1;
    let cmp_node_id = param_a + 2;
    let precond_id = param_a + 3;
    let ret_id = param_a + 4;

    let body = batch_mutate(
        &app,
        pid,
        json!([
            // Const(0)
            {
                "type": "InsertNode",
                "op": {"Core": {"Const": {"value": {"I32": 0}}}},
                "owner": func_id
            },
            // Compare(Ge)
            {
                "type": "InsertNode",
                "op": {"Core": {"Compare": {"op": "Ge"}}},
                "owner": func_id
            },
            // Precondition
            {
                "type": "InsertNode",
                "op": {"Core": {"Precondition": {"message": "a must be non-negative"}}},
                "owner": func_id
            },
            // Return
            {
                "type": "InsertNode",
                "op": {"Core": "Return"},
                "owner": func_id
            },
            // Data edges: a -> Compare port 0, Const(0) -> Compare port 1
            {
                "type": "AddEdge",
                "from": param_a, "to": cmp_node_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": const_zero_id, "to": cmp_node_id,
                "source_port": 0, "target_port": 1,
                "value_type": 3
            },
            // Data edge: Compare -> Precondition port 0 (condition)
            {
                "type": "AddEdge",
                "from": cmp_node_id, "to": precond_id,
                "source_port": 0, "target_port": 0,
                "value_type": 0
            },
            // Data edge: a -> Return port 0
            {
                "type": "AddEdge",
                "from": param_a, "to": ret_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            },
            // Control edge: Precondition -> Return (ensures contract checked before return)
            {
                "type": "AddControlEdge",
                "from": precond_id, "to": ret_id,
                "branch_index": null
            }
        ]),
    )
    .await;
    assert!(
        body["valid"].as_bool().unwrap(),
        "function build should be valid: {:?}",
        body
    );
    assert!(
        body["committed"].as_bool().unwrap(),
        "should commit: {:?}",
        body
    );

    // Run property test with seed containing a known negative
    let (status, test_body) = post_json(
        &app,
        &format!("/programs/{}/property-test", pid),
        json!({
            "function_id": func_id,
            "seeds": [[-1], [0], [5]],
            "iterations": 50,
            "random_seed": 42,
            "trace_failures": true
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "property test request failed: {:?}",
        test_body
    );

    // Should have total_run = 3 seeds + 50 random = 53
    assert_eq!(test_body["total_run"].as_u64().unwrap(), 53);

    // Should find at least the seed(-1) failure
    assert!(
        test_body["failed"].as_u64().unwrap() >= 1,
        "expected at least 1 failure"
    );

    let failures = test_body["failures"].as_array().unwrap();
    assert!(!failures.is_empty(), "should have failure details");

    // First failure should be from seed input -1
    // Values are serialized as {"I32": -1} (serde Value enum format)
    let first_failure = &failures[0];
    assert_eq!(
        first_failure["inputs"][0]["I32"].as_i64().unwrap(),
        -1,
        "first failure input should be -1"
    );

    // Violation details
    let violation = &first_failure["violation"];
    assert_eq!(violation["kind"].as_str().unwrap(), "precondition");
    assert_eq!(
        violation["message"].as_str().unwrap(),
        "a must be non-negative"
    );
    assert!(
        violation["contract_node"].is_number(),
        "should have contract_node"
    );
    assert!(
        violation["function_id"].is_number(),
        "should have function_id"
    );
    assert!(
        violation["counterexample"].is_array(),
        "should have counterexample"
    );

    // Trace should be present (trace_failures=true)
    assert!(first_failure["trace"].is_array(), "trace should be present");
    assert!(
        !first_failure["trace"].as_array().unwrap().is_empty(),
        "trace should have entries"
    );

    // Random seed should be returned for reproducibility
    assert_eq!(test_body["random_seed"].as_u64().unwrap(), 42);
}

/// Test 15: Property test with all valid inputs reports zero failures.
#[tokio::test]
async fn cntr05_property_test_all_pass() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Create simple_fn(a: i32) -> i32 with NO contracts
    let func_id = add_typed_function(&app, pid, "simple_fn", json!([["a", 3]]), 3).await;

    let param_a = insert_param(&app, pid, func_id, 0).await;

    let ret_id = param_a + 1;
    let body = batch_mutate(
        &app,
        pid,
        json!([
            {
                "type": "InsertNode",
                "op": {"Core": "Return"},
                "owner": func_id
            },
            {
                "type": "AddEdge",
                "from": param_a, "to": ret_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            }
        ]),
    )
    .await;
    assert!(
        body["valid"].as_bool().unwrap(),
        "function build should be valid: {:?}",
        body
    );

    let (status, test_body) = post_json(
        &app,
        &format!("/programs/{}/property-test", pid),
        json!({
            "function_id": func_id,
            "seeds": [[0], [42], [-1]],
            "iterations": 20,
            "random_seed": 99
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "property test request failed: {:?}",
        test_body
    );

    assert_eq!(test_body["total_run"].as_u64().unwrap(), 23);
    assert_eq!(test_body["passed"].as_u64().unwrap(), 23);
    assert_eq!(test_body["failed"].as_u64().unwrap(), 0);
    assert!(test_body["failures"].as_array().unwrap().is_empty());
}

// ===========================================================================
// STORE-05: Dirty status query (incremental compilation)
// ===========================================================================

/// Test 16: Dirty status returns all functions as dirty when no prior compilation exists.
///
/// Creates a simple program with one function and queries the dirty endpoint.
/// Since no compilation has occurred, all functions should be reported as dirty.
#[tokio::test]
async fn store05_dirty_status_all_dirty_without_prior_compile() {
    let app = test_app();
    let pid = setup_program(&app).await;

    // Create a function
    let func_id = add_function(&app, pid, "my_func").await;

    // Add a node so the function is valid
    let _ret_id = func_id + 1;
    let body = batch_mutate(
        &app,
        pid,
        json!([
            {
                "type": "InsertNode",
                "op": {"Core": "Return"},
                "owner": func_id
            }
        ]),
    )
    .await;
    assert!(
        body["valid"].as_bool().unwrap(),
        "function build should be valid: {:?}",
        body
    );

    // Query dirty status
    let (status, dirty_body) = get_json(&app, &format!("/programs/{}/dirty", pid)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "dirty status request failed: {:?}",
        dirty_body
    );

    // Should report needs_recompilation
    assert!(
        dirty_body["needs_recompilation"].as_bool().unwrap(),
        "should need recompilation"
    );

    // Should have dirty_functions with at least 1 entry
    let dirty_functions = dirty_body["dirty_functions"].as_array().unwrap();
    assert!(!dirty_functions.is_empty(), "should have dirty functions");

    // Each dirty function should have function_id, function_name, reason
    let first = &dirty_functions[0];
    assert!(first["function_id"].is_number(), "should have function_id");
    assert!(
        first["function_name"].is_string(),
        "should have function_name"
    );
    assert_eq!(
        first["reason"].as_str().unwrap(),
        "no_prior_compilation",
        "reason should be no_prior_compilation"
    );

    // Should have no cached functions
    let cached = dirty_body["cached_functions"].as_array().unwrap();
    assert!(cached.is_empty(), "should have no cached functions");
}

// ===========================================================================
// PHASE 08: Dual-layer semantic architecture
// ===========================================================================

#[tokio::test]
async fn phase08_semantic_query_exposes_summary_and_embeddings() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let _func_id = add_function(&app, pid, "semantic_fn").await;

    // Flush propagation so function summaries/embeddings are refreshed.
    let (status, flush_body) = post_json(
        &app,
        &format!("/programs/{}/verify/flush", pid),
        json!({ "dry_run": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "flush failed: {:?}", flush_body);

    let (status, semantic_before) = post_json(
        &app,
        &format!("/programs/{}/semantic", pid),
        json!({ "include_embeddings": true }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "semantic query failed: {:?}",
        semantic_before
    );

    let nodes = semantic_before["nodes"].as_array().unwrap();
    let function_node = nodes
        .iter()
        .find(|n| n["kind"] == "function" && n["label"] == "semantic_fn")
        .expect("semantic function node should exist");
    assert!(function_node["summary_checksum"].is_string());
    assert!(function_node["has_node_embedding"].as_bool().unwrap());
    assert!(function_node["node_embedding"].is_array());
    let checksum_before = function_node["summary_checksum"]
        .as_str()
        .unwrap()
        .to_string();

    // Reload and verify deterministic summary persistence.
    let (status, reload_body) =
        post_json(&app, &format!("/programs/{}/load", pid), json!({})).await;
    assert_eq!(status, StatusCode::OK, "reload failed: {:?}", reload_body);

    let (status, semantic_after) = post_json(
        &app,
        &format!("/programs/{}/semantic", pid),
        json!({ "include_embeddings": true }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "semantic query after reload failed: {:?}",
        semantic_after
    );
    let nodes_after = semantic_after["nodes"].as_array().unwrap();
    let function_node_after = nodes_after
        .iter()
        .find(|n| n["kind"] == "function" && n["label"] == "semantic_fn")
        .expect("semantic function node should exist after reload");
    let checksum_after = function_node_after["summary_checksum"].as_str().unwrap();
    assert_eq!(checksum_before, checksum_after);
}

#[tokio::test]
async fn phase08_flush_is_idempotent_when_queue_unchanged() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "flush_idempotent").await;
    let _node = insert_param(&app, pid, func_id, 0).await;

    let (status, first_flush) = post_json(
        &app,
        &format!("/programs/{}/verify/flush", pid),
        json!({ "dry_run": false }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "first flush failed: {:?}",
        first_flush
    );
    assert!(first_flush["processed_events"].as_u64().unwrap() >= 1);

    let (status, second_flush) = post_json(
        &app,
        &format!("/programs/{}/verify/flush", pid),
        json!({ "dry_run": false }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "second flush failed: {:?}",
        second_flush
    );
    assert_eq!(second_flush["processed_events"].as_u64().unwrap(), 0);
    assert_eq!(second_flush["applied_events"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn phase08_unresolved_conflict_returns_structured_diagnostic() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_function(&app, pid, "conflict_fn").await;
    let node_id = insert_param(&app, pid, func_id, 0).await;

    // Clear naturally queued events first.
    let (status, clear_flush) = post_json(
        &app,
        &format!("/programs/{}/verify/flush", pid),
        json!({ "dry_run": false }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "clear flush failed: {:?}",
        clear_flush
    );

    let (status, conflict_body) = post_json(
        &app,
        &format!("/programs/{}/verify/flush", pid),
        json!({
            "dry_run": false,
            "events": [
                {
                    "kind": "compute.node_modified",
                    "function_id": func_id,
                    "node_id": node_id,
                    "op_kind": "Parameter"
                },
                {
                    "kind": "semantic.function_signature_changed",
                    "function_id": func_id
                }
            ]
        }),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT, "expected conflict response");
    assert_eq!(conflict_body["error"]["code"], "CONFLICT");
    assert!(conflict_body["error"]["details"].is_array());
    let details = conflict_body["error"]["details"].as_array().unwrap();
    assert!(!details.is_empty());
    assert_eq!(
        details[0]["precedence"].as_str().unwrap(),
        "diagnostic-required"
    );
}

// ===========================================================================
// PHASE 09: Human observability
// ===========================================================================

#[tokio::test]
async fn phase09_observability_graph_exposes_layers_boundaries_and_cross_links() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let func_id = add_typed_function(&app, pid, "sum_alpha", json!([["a", 3], ["b", 3]]), 3).await;
    let param_a = insert_param(&app, pid, func_id, 0).await;
    let param_b = insert_param(&app, pid, func_id, 1).await;

    let add_node_id = param_b + 1;
    let ret_node_id = param_b + 2;
    let body = batch_mutate(
        &app,
        pid,
        json!([
            {
                "type": "InsertNode",
                "op": {"Core": {"BinaryArith": {"op": "Add"}}},
                "owner": func_id
            },
            {
                "type": "InsertNode",
                "op": {"Core": "Return"},
                "owner": func_id
            },
            {
                "type": "AddEdge",
                "from": param_a, "to": add_node_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": param_b, "to": add_node_id,
                "source_port": 0, "target_port": 1,
                "value_type": 3
            },
            {
                "type": "AddEdge",
                "from": add_node_id, "to": ret_node_id,
                "source_port": 0, "target_port": 0,
                "value_type": 3
            }
        ]),
    )
    .await;
    assert!(
        body["valid"].as_bool().unwrap(),
        "batch invalid: {:?}",
        body
    );
    assert!(
        body["committed"].as_bool().unwrap(),
        "batch not committed: {:?}",
        body
    );

    let (status, graph_a) = get_json(&app, &format!("/programs/{}/observability/graph", pid)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "graph request failed: {:?}",
        graph_a
    );

    let nodes = graph_a["nodes"].as_array().unwrap();
    let edges = graph_a["edges"].as_array().unwrap();
    let groups = graph_a["groups"].as_array().unwrap();
    assert!(
        !groups.is_empty(),
        "groups should include function boundary metadata"
    );
    assert!(
        nodes.iter().any(|n| n["layer"] == "semantic"),
        "should include semantic layer nodes"
    );
    assert!(
        nodes.iter().any(|n| n["layer"] == "compute"),
        "should include compute layer nodes"
    );
    assert!(
        edges
            .iter()
            .any(|e| e["cross_layer"].as_bool().unwrap_or(false)),
        "should include cross-layer links"
    );
    assert!(
        edges
            .iter()
            .any(|e| e["edge_kind"] == "data" && e["value_type"].is_number()),
        "should include typed data edges"
    );

    let (status, graph_b) = get_json(&app, &format!("/programs/{}/observability/graph", pid)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        graph_a, graph_b,
        "observability projection must be deterministic"
    );
}

#[tokio::test]
async fn phase09_observability_query_handles_ambiguity_and_low_confidence_fallback() {
    let app = test_app();
    let pid = setup_program(&app).await;
    let _sum_alpha =
        add_typed_function(&app, pid, "sum_alpha", json!([["x", 3], ["y", 3]]), 3).await;
    let _sum_beta = add_typed_function(&app, pid, "sum_beta", json!([["x", 3], ["y", 3]]), 3).await;

    let (status, flush_body) = post_json(
        &app,
        &format!("/programs/{}/verify/flush", pid),
        json!({ "dry_run": false }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "flush failed: {:?}", flush_body);

    let (status, ambiguous_body) = post_json(
        &app,
        &format!("/programs/{}/observability/query", pid),
        json!({ "query": "sum", "max_results": 5 }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "observability query failed: {:?}",
        ambiguous_body
    );
    assert!(
        ambiguous_body["ambiguous"].as_bool().unwrap(),
        "query should be flagged ambiguous: {:?}",
        ambiguous_body
    );
    assert!(ambiguous_body["ambiguity_prompt"].is_string());
    let interpretations = ambiguous_body["interpretations"].as_array().unwrap();
    assert!(
        interpretations.len() >= 2,
        "ambiguous query should provide interpretation choices"
    );
    let selected = interpretations[0]["candidate_id"].as_str().unwrap();

    let (status, disambiguated_body) = post_json(
        &app,
        &format!("/programs/{}/observability/query", pid),
        json!({
            "query": "sum",
            "selected_candidate_id": selected,
            "max_results": 5
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "disambiguated query failed");
    assert!(
        !disambiguated_body["ambiguous"].as_bool().unwrap(),
        "selected candidate should resolve ambiguity"
    );
    let results = disambiguated_body["results"].as_array().unwrap();
    assert!(!results.is_empty(), "resolved query should return results");
    let first = &results[0];
    assert!(first["summary"].is_object());
    assert!(first["relationships"].is_object());
    assert!(first["contracts"].is_object());

    let (status, fallback_body) = post_json(
        &app,
        &format!("/programs/{}/observability/query", pid),
        json!({ "query": "quasar neutrino graph arcana", "max_results": 5 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "fallback query failed");
    assert!(
        fallback_body["low_confidence"].as_bool().unwrap(),
        "unmatched query should be low-confidence"
    );
    assert!(fallback_body["fallback_reason"].is_string());
    let fallback_results = fallback_body["results"].as_array().unwrap();
    assert!(
        !fallback_results.is_empty(),
        "low-confidence query should return nearest related results"
    );
}

#[tokio::test]
async fn phase09_observability_routes_serve_static_ui_assets() {
    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, html) = get_text(&app, &format!("/programs/{}/observability", pid)).await;
    assert_eq!(status, StatusCode::OK, "ui index not served");
    assert!(html.contains("lmlang Observability"));
    assert!(html.contains(&format!("/programs/{}/observability/app.js", pid)));

    let (status, js) = get_text(&app, &format!("/programs/{}/observability/app.js", pid)).await;
    assert_eq!(status, StatusCode::OK, "app.js not served");
    assert!(js.contains("runQuery"));
    assert!(js.contains("renderGraph"));

    let (status, css) =
        get_text(&app, &format!("/programs/{}/observability/styles.css", pid)).await;
    assert_eq!(status, StatusCode::OK, "styles.css not served");
    assert!(css.contains(".graph-node"));
    assert!(css.contains(".edge-cross"));
}

// ===========================================================================
// PHASE 10: Unified dashboard shell
// ===========================================================================

#[tokio::test]
async fn phase10_dashboard_routes_serve_shell_and_assets() {
    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, root_html) = get_text(&app, "/dashboard").await;
    assert_eq!(status, StatusCode::OK, "top-level dashboard not served");
    assert!(root_html.contains("Unified Dashboard"));
    assert!(root_html.contains("Create Project"));
    assert!(root_html.contains("Create Hello World Scaffold"));
    assert!(root_html.contains("Assign To Project"));
    assert!(root_html.contains("OpenRouter"));
    assert!(root_html.contains("Save Agent Config"));
    assert!(root_html.contains("First-Time AI Setup"));
    assert!(root_html.contains("Complete Setup"));
    assert!(root_html.contains("Start Build"));
    assert!(root_html.contains("Send"));
    assert!(root_html.contains("data-initial-program-id=\"\""));

    let (status, html) = get_text(&app, &format!("/programs/{}/dashboard", pid)).await;
    assert_eq!(status, StatusCode::OK, "dashboard index not served");
    assert!(html.contains("Unified Dashboard"));
    assert!(html.contains(&format!("data-initial-program-id=\"{}\"", pid)));
    assert!(html.contains("projectList"));
    assert!(html.contains("projectAgentList"));
    assert!(html.contains("chatLog"));

    let (status, js) = get_text(&app, "/dashboard/app.js").await;
    assert_eq!(status, StatusCode::OK, "dashboard app.js not served");
    assert!(js.contains("/programs"));
    assert!(js.contains("/mutations"));
    assert!(js.contains("/verify"));
    assert!(js.contains("/observability/query"));
    assert!(js.contains("/dashboard/ai/chat"));
    assert!(js.contains("lmlang.dashboard.first_time_setup.v1"));
    assert!(js.contains("hello_world"));
    assert!(js.contains("/agents/register"));
    assert!(js.contains("/agents/${agentId}/config"));
    assert!(js.contains("/agents/${state.selectedProjectAgentId}/start"));
    assert!(js.contains("/agents/${state.selectedProjectAgentId}/stop"));
    assert!(js.contains("/dashboard/ai/chat"));
    assert!(js.contains("/observability"));
    assert!(js.contains("Create project failed"));
    assert!(js.contains("Start build failed"));

    let (status, css) = get_text(&app, "/dashboard/styles.css").await;
    assert_eq!(status, StatusCode::OK, "dashboard styles.css not served");
    assert!(css.contains(".page-shell"));
    assert!(css.contains(".workspace"));
    assert!(css.contains(".panel-chat"));
    assert!(css.contains(".chat-log"));
}

#[tokio::test]
async fn phase10_dashboard_project_agent_lifecycle_endpoints_work() {
    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, register) =
        post_json(&app, "/agents/register", json!({ "name": "builder" })).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "register agent failed: {:?}",
        register
    );
    let agent_id = register["agent_id"]
        .as_str()
        .expect("agent_id must be a string UUID");

    let (status, assign) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/assign", pid, agent_id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign failed: {:?}", assign);
    assert_eq!(assign["success"], json!(true));
    assert_eq!(assign["session"]["run_status"], json!("idle"));

    let (status, start) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/start", pid, agent_id),
        json!({ "goal": "build parser" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "start failed: {:?}", start);
    assert_eq!(start["session"]["run_status"], json!("running"));
    assert_eq!(start["session"]["active_goal"], json!("build parser"));

    let (status, chat_create) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/chat", pid, agent_id),
        json!({ "message": "create hello world program" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "chat create hello world failed: {:?}",
        chat_create
    );
    assert_eq!(chat_create["success"], json!(true));
    assert!(chat_create["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Action result:"));
    assert!(chat_create["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Hello world scaffold ready"));
    let create_transcript = chat_create["transcript"].as_array().unwrap();
    assert!(create_transcript.len() >= 3);
    assert!(create_transcript.iter().any(|entry| {
        entry["content"]
            .as_str()
            .unwrap_or_default()
            .contains("Action result:")
    }));

    let (status, chat_compile) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/chat", pid, agent_id),
        json!({ "message": "compile program" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "chat compile failed: {:?}",
        chat_compile
    );
    assert!(chat_compile["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Action result:"));
    assert!(chat_compile["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Compiled hello_world"));

    let (status, chat_run) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/chat", pid, agent_id),
        json!({ "message": "run program" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "chat run failed: {:?}", chat_run);
    assert!(chat_run["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Action result:"));
    assert!(chat_run["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Program executed"));

    let (status, stop) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/stop", pid, agent_id),
        json!({ "reason": "manual stop" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "stop failed: {:?}", stop);
    assert_eq!(stop["session"]["run_status"], json!("stopped"));

    let (status, detail) = get_json(&app, &format!("/programs/{}/agents/{}", pid, agent_id)).await;
    assert_eq!(status, StatusCode::OK, "detail failed: {:?}", detail);
    assert_eq!(detail["session"]["run_status"], json!("stopped"));
    assert!(detail["transcript"].as_array().unwrap().len() >= 8);

    let (status, listing) = get_json(&app, &format!("/programs/{}/agents", pid)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "list project agents failed: {:?}",
        listing
    );
    assert_eq!(listing["program_id"], json!(pid));
    assert_eq!(listing["agents"].as_array().unwrap().len(), 1);

    let (status, query) = post_json(
        &app,
        &format!("/programs/{}/observability/query", pid),
        json!({
            "query": "hello world",
            "max_results": 5
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "observability query failed: {:?}",
        query
    );
    assert!(query["results"].is_array());
}

#[tokio::test]
async fn phase10_agent_llm_config_endpoints_work() {
    let app = test_app();

    let (status, register) = post_json(
        &app,
        "/agents/register",
        json!({
            "name": "builder",
            "provider": "openrouter",
            "model": "openai/gpt-4o-mini",
            "api_base_url": "https://openrouter.ai/api/v1",
            "api_key": "test-key",
            "system_prompt": "You are a build assistant."
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "register with config failed: {:?}",
        register
    );
    let agent_id = register["agent_id"].as_str().unwrap();
    assert_eq!(register["llm"]["provider"], json!("openrouter"));
    assert_eq!(register["llm"]["model"], json!("openai/gpt-4o-mini"));
    assert_eq!(
        register["llm"]["api_base_url"],
        json!("https://openrouter.ai/api/v1")
    );
    assert_eq!(register["llm"]["api_key_configured"], json!(true));

    let (status, detail) = get_json(&app, &format!("/agents/{}", agent_id)).await;
    assert_eq!(status, StatusCode::OK, "get agent failed: {:?}", detail);
    assert_eq!(detail["agent"]["llm"]["provider"], json!("openrouter"));

    let (status, update) = post_json(
        &app,
        &format!("/agents/{}/config", agent_id),
        json!({
            "provider": "openai_compatible",
            "model": "gpt-4.1-mini",
            "api_base_url": "https://api.openai.com/v1",
            "api_key": "other-test-key",
            "system_prompt": "be concise"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "update config failed: {:?}", update);
    assert_eq!(update["success"], json!(true));
    assert_eq!(
        update["agent"]["llm"]["provider"],
        json!("openai_compatible")
    );
    assert_eq!(
        update["agent"]["llm"]["api_base_url"],
        json!("https://api.openai.com/v1")
    );
    assert_eq!(update["agent"]["llm"]["api_key_configured"], json!(true));
}

#[tokio::test]
async fn phase10_agent_llm_config_persists_across_restart() {
    let db_path = temp_db_path("lmlang_agent_config_persist");
    let app = test_app_with_db(&db_path);

    let (status, register) = post_json(
        &app,
        "/agents/register",
        json!({
            "name": "persisted-builder",
            "provider": "openrouter",
            "model": "openai/gpt-4o-mini",
            "api_base_url": "https://openrouter.ai/api/v1",
            "api_key": "persisted-test-key",
            "system_prompt": "Persist me."
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "register with persistence failed: {:?}",
        register
    );
    let agent_id = register["agent_id"].as_str().unwrap().to_string();
    drop(app);

    let app_restarted = test_app_with_db(&db_path);
    let (status, listing) = get_json(&app_restarted, "/agents").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "list after restart failed: {:?}",
        listing
    );

    let agent = listing["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["agent_id"] == json!(agent_id))
        .expect("persisted agent should exist after restart");
    assert_eq!(agent["name"], json!("persisted-builder"));
    assert_eq!(agent["llm"]["provider"], json!("openrouter"));
    assert_eq!(agent["llm"]["api_key_configured"], json!(true));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-shm", &db_path));
    let _ = std::fs::remove_file(format!("{}-wal", &db_path));
}

#[tokio::test]
async fn phase10_start_build_runs_autonomous_hello_world_scaffold() {
    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, register) =
        post_json(&app, "/agents/register", json!({ "name": "auto-builder" })).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "register agent failed: {:?}",
        register
    );
    let agent_id = register["agent_id"].as_str().unwrap();

    let (status, assign) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/assign", pid, agent_id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign failed: {:?}", assign);

    let (status, start) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/start", pid, agent_id),
        json!({ "goal": "hello world scaffold" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "start failed: {:?}", start);

    let mut autonomous_hit = false;
    for _ in 0..30 {
        let (status, detail) =
            get_json(&app, &format!("/programs/{}/agents/{}", pid, agent_id)).await;
        assert_eq!(status, StatusCode::OK, "detail failed: {:?}", detail);

        if detail["transcript"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("Autonomous step `create hello world program` complete")
            })
        {
            autonomous_hit = true;
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(
        autonomous_hit,
        "expected autonomous loop to scaffold hello world without chat turn"
    );

    let mut reached_idle = false;
    for _ in 0..12 {
        let (_, detail) = get_json(&app, &format!("/programs/{}/agents/{}", pid, agent_id)).await;
        if detail["session"]["run_status"] == json!("idle") {
            reached_idle = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(
        reached_idle,
        "autonomous run should settle to idle once hello world scaffold goal is satisfied"
    );
}

#[tokio::test]
async fn phase10_dashboard_ai_chat_orchestrates_end_to_end() {
    let app = test_app();

    let (status, create_project) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "create project hello-world"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create project via ai chat failed: {:?}",
        create_project
    );
    let program_id = create_project["selected_program_id"].as_i64().unwrap();

    let (status, register_agent) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "register agent builder provider openrouter model openai/gpt-4o-mini api key test-key",
            "selected_program_id": program_id
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "register agent via ai chat failed: {:?}",
        register_agent
    );
    let agent_id = register_agent["selected_agent_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, assign) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "assign agent",
            "selected_program_id": program_id,
            "selected_agent_id": agent_id
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "assign via ai chat failed: {:?}",
        assign
    );
    assert_eq!(
        assign["selected_project_agent_id"].as_str().unwrap(),
        agent_id
    );

    let (status, start) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "start build hello world bootstrap",
            "selected_program_id": program_id,
            "selected_agent_id": agent_id,
            "selected_project_agent_id": agent_id
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "start via ai chat failed: {:?}",
        start
    );

    let (status, create_hw) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "create hello world program",
            "selected_program_id": program_id,
            "selected_agent_id": agent_id,
            "selected_project_agent_id": agent_id
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create hello world via ai chat failed: {:?}",
        create_hw
    );
    assert!(create_hw["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Hello world scaffold ready"));

    let (status, compile) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "compile program",
            "selected_program_id": program_id,
            "selected_agent_id": agent_id,
            "selected_project_agent_id": agent_id
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "compile via ai chat failed: {:?}",
        compile
    );
    assert!(compile["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Compiled hello_world"));

    let (status, run) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "run program",
            "selected_program_id": program_id,
            "selected_agent_id": agent_id,
            "selected_project_agent_id": agent_id
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "run via ai chat failed: {:?}", run);
    assert!(run["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Program executed"));
    assert!(run["transcript"].is_array());

    let (status, query) = post_json(
        &app,
        &format!("/programs/{}/observability/query", program_id),
        json!({
            "query": "hello world",
            "max_results": 5
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "observability query failed: {:?}",
        query
    );
    assert!(query["results"].is_array());
}

#[tokio::test]
async fn phase10_dashboard_and_observe_routes_coexist_with_reuse_contract() {
    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, dashboard_html) = get_text(&app, "/dashboard").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "dashboard route should be available"
    );
    assert!(dashboard_html.contains("Unified Dashboard"));
    assert!(dashboard_html.contains("/dashboard/app.js"));

    let (status, observe_html) = get_text(&app, &format!("/programs/{}/observability", pid)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "observe route should remain available"
    );
    assert!(observe_html.contains("lmlang Observability"));

    let (status, js) = get_text(&app, "/dashboard/app.js").await;
    assert_eq!(status, StatusCode::OK, "dashboard js should be available");
    assert!(js.contains("/programs/${state.selectedProgramId}/observability"));
}

// ===========================================================================
// PHASE 14: Planner contract routing
// ===========================================================================

#[tokio::test]
async fn phase14_program_agent_chat_routes_non_command_to_planner() {
    let planner_response = r#"{
        "version": "2026-02-19",
        "goal": "build a simple calculator",
        "actions": [
            {
                "type": "inspect",
                "request": {
                    "query": "calculator requirements",
                    "max_results": 5
                }
            },
            {
                "type": "verify",
                "request": { "scope": "Full" }
            }
        ]
    }"#;
    let (base_url, requests, server) = start_mock_planner_server(planner_response).await;

    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, register) = post_json(
        &app,
        "/agents/register",
        json!({
            "name": "planner-agent",
            "provider": "openai_compatible",
            "model": "planner-test",
            "api_base_url": base_url,
            "api_key": "test-key"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register failed: {:?}", register);
    let agent_id = register["agent_id"].as_str().unwrap();

    let (status, assign) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/assign", pid, agent_id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign failed: {:?}", assign);

    let (status, chat) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/chat", pid, agent_id),
        json!({
            "message": "build a simple calculator with add and subtract operations"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "planner chat failed: {:?}",
        chat
    );
    assert_eq!(chat["planner"]["status"], json!("accepted"));
    assert_eq!(chat["planner"]["version"], json!("2026-02-19"));
    assert!(
        chat["planner"]["actions"].as_array().unwrap().len() >= 2,
        "expected multi-step planner actions: {:?}",
        chat["planner"]
    );
    assert!(chat["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Planner accepted"));

    let requests_guard = requests.lock().unwrap();
    assert!(!requests_guard.is_empty(), "mock planner received no requests");
    assert_eq!(
        requests_guard[0]["response_format"]["type"],
        json!("json_object")
    );

    server.abort();
}

#[tokio::test]
async fn phase14_program_agent_chat_returns_structured_failure_for_invalid_planner_json() {
    let (base_url, _requests, server) = start_mock_planner_server("not-json-at-all").await;

    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, register) = post_json(
        &app,
        "/agents/register",
        json!({
            "name": "planner-agent",
            "provider": "openai_compatible",
            "model": "planner-test",
            "api_base_url": base_url,
            "api_key": "test-key"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "register failed: {:?}", register);
    let agent_id = register["agent_id"].as_str().unwrap();

    let (status, assign) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/assign", pid, agent_id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign failed: {:?}", assign);

    let (status, chat) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/chat", pid, agent_id),
        json!({
            "message": "build a state machine for ticket approval"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "chat failed: {:?}", chat);
    assert_eq!(chat["planner"]["status"], json!("failed"));
    assert_eq!(
        chat["planner"]["failure"]["code"],
        json!("planner_invalid_json")
    );
    assert!(chat["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("planner_invalid_json"));

    server.abort();
}

#[tokio::test]
async fn phase14_dashboard_ai_chat_surfaces_planner_payload() {
    let planner_response = r#"{
        "version": "2026-02-19",
        "goal": "build calculator workflow",
        "actions": [
            {
                "type": "inspect",
                "request": {
                    "query": "calculator plan",
                    "max_results": 4
                }
            },
            {
                "type": "verify",
                "request": { "scope": "Local" }
            }
        ]
    }"#;
    let (base_url, _requests, server) = start_mock_planner_server(planner_response).await;

    let app = test_app();

    let (status, create_project) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "create project phase14-planner-dashboard"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "create project failed: {:?}",
        create_project
    );
    let program_id = create_project["selected_program_id"].as_i64().unwrap();

    let register_message = format!(
        "register agent planner provider openai_compatible model planner-test base url {} api key test-key",
        base_url
    );
    let (status, register_agent) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": register_message,
            "selected_program_id": program_id
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "register agent failed: {:?}",
        register_agent
    );
    let agent_id = register_agent["selected_agent_id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, assign) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "assign agent",
            "selected_program_id": program_id,
            "selected_agent_id": agent_id
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign failed: {:?}", assign);

    let (status, chat) = post_json(
        &app,
        "/dashboard/ai/chat",
        json!({
            "message": "build calculator workflow from this prompt",
            "selected_program_id": program_id,
            "selected_agent_id": agent_id,
            "selected_project_agent_id": agent_id
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "dashboard chat failed: {:?}", chat);
    assert_eq!(chat["planner"]["status"], json!("accepted"));
    assert!(
        chat["planner"]["actions"].as_array().unwrap().len() >= 2,
        "expected planner actions in dashboard response"
    );

    server.abort();
}

#[tokio::test]
async fn phase14_explicit_command_prompt_keeps_deterministic_hello_world_path() {
    let app = test_app();
    let pid = setup_program(&app).await;

    let (status, register) =
        post_json(&app, "/agents/register", json!({ "name": "command-agent" })).await;
    assert_eq!(status, StatusCode::OK, "register failed: {:?}", register);
    let agent_id = register["agent_id"].as_str().unwrap();

    let (status, assign) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/assign", pid, agent_id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "assign failed: {:?}", assign);

    let (status, chat) = post_json(
        &app,
        &format!("/programs/{}/agents/{}/chat", pid, agent_id),
        json!({
            "message": "create hello world program"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "chat failed: {:?}", chat);
    assert!(chat["reply"]
        .as_str()
        .unwrap_or_default()
        .contains("Hello world scaffold ready"));
    assert!(
        chat["planner"].is_null(),
        "command-path response should not include planner payload: {:?}",
        chat
    );
}
