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
use axum::http::{Request, StatusCode};
use axum::Router;
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
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes).unwrap_or(json!(null));
    (status, json)
}

/// Sends a GET request and returns (status, json).
async fn get_json(
    app: &Router,
    path: &str,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes).unwrap_or(json!(null));
    (status, json)
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
    assert!(body["valid"].as_bool().unwrap(), "add function validation failed: {:?}", body);
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
    assert_eq!(status, StatusCode::OK, "add typed function failed: {:?}", body);
    assert!(body["valid"].as_bool().unwrap(), "add typed function validation failed: {:?}", body);
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
    assert!(body["valid"].as_bool().unwrap(), "insert const validation failed: {:?}", body);
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
    assert!(body["valid"].as_bool().unwrap(), "insert param validation failed: {:?}", body);
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
    let func_id = add_typed_function(
        &app, pid, "add",
        json!([["a", 3], ["b", 3]]),
        3,
    ).await;

    // Add params (pass validation alone)
    let param_a = insert_param(&app, pid, func_id, 0).await;
    let param_b = insert_param(&app, pid, func_id, 1).await;

    // Add BinaryArith(Add) + edges in a single batch (BinaryArith needs inputs)
    let body = batch_mutate(&app, pid, json!([
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
    ])).await;

    assert!(body["valid"].as_bool().unwrap(), "batch should be valid: {:?}", body);
    assert!(body["committed"].as_bool().unwrap(), "batch should commit");
    assert!(!body["created"].as_array().unwrap().is_empty(), "should have created entities");

    // GET overview and verify
    let (status, overview) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    assert_eq!(status, StatusCode::OK);
    // 3 nodes: param_a, param_b, add_node
    assert_eq!(overview["node_count"].as_u64().unwrap(), 3);
    assert!(overview["edge_count"].as_u64().unwrap() >= 2);
    let functions = overview["functions"].as_array().unwrap();
    assert!(!functions.is_empty(), "expected at least one function in overview");
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
    assert!(body["valid"].as_bool().unwrap(), "dry_run should report valid=true");
    assert!(!body["committed"].as_bool().unwrap(), "dry_run should NOT commit");

    let (_, overview_after) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_after = overview_after["node_count"].as_u64().unwrap();
    assert_eq!(count_before, count_after, "dry_run should not change node count");
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
    assert!(!body["valid"].as_bool().unwrap(), "batch with invalid mutation should be invalid");
    assert!(!body["committed"].as_bool().unwrap(), "batch should NOT commit on failure");
    assert!(!body["errors"].as_array().unwrap().is_empty(), "should have errors");

    // Verify graph unchanged (first mutation was NOT applied)
    let (_, overview_after) = get_json(&app, &format!("/programs/{}/overview", pid)).await;
    let count_after = overview_after["node_count"].as_u64().unwrap();
    assert_eq!(count_before, count_after, "failed batch must not change graph");
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
    assert!(body["incoming_edges"].is_array(), "full detail should include incoming_edges");
    assert!(body["outgoing_edges"].is_array(), "full detail should include outgoing_edges");
    // node_b should have 1 incoming edge from node_a
    assert!(!body["incoming_edges"].as_array().unwrap().is_empty(), "should have incoming edge");
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
    assert!(body["valid"].as_bool().unwrap(), "chain edges should be valid: {:?}", body);

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

    assert!(node_ids.contains(&(a as u64)), "A should be in 2-hop neighborhood");
    assert!(node_ids.contains(&(b as u64)), "B should be in 2-hop neighborhood");
    assert!(node_ids.contains(&(c as u64)), "C should be in 2-hop neighborhood");
    assert!(!node_ids.contains(&(d as u64)), "D should NOT be in 2-hop neighborhood from A");
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
    let func_id = add_typed_function(
        &app, pid, "bad_add",
        json!([["a", 3], ["b", 6]]),
        3,
    ).await;

    // Add params (pass validation alone)
    let param_a = insert_param(&app, pid, func_id, 0).await;
    let param_b = insert_param(&app, pid, func_id, 1).await;

    // Batch: Add BinaryArith(Add) + edges with type mismatch (i32 and f64)
    let body = batch_mutate(&app, pid, json!([
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
    ])).await;

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
        assert!(!verify_body["valid"].as_bool().unwrap(), "should detect type mismatch");

        let errors = verify_body["errors"].as_array().unwrap();
        assert!(!errors.is_empty(), "should have at least one error");

        let error = &errors[0];
        assert_eq!(error["code"].as_str().unwrap(), "TYPE_MISMATCH");
        assert!(error["details"].is_object(), "should have structured details");
        let details = &error["details"];
        assert!(details["source_node"].is_number(), "should have source_node");
        assert!(details["target_node"].is_number(), "should have target_node");
        assert!(details["expected_type"].is_number(), "should have expected_type");
        assert!(details["actual_type"].is_number(), "should have actual_type");
    } else {
        // Batch was rejected at mutation time -- check batch errors
        assert!(!body["valid"].as_bool().unwrap(), "batch with mismatch should be invalid");
        let errors = body["errors"].as_array().unwrap();
        assert!(!errors.is_empty(), "should have errors from batch rejection");
        // Verify the error is a type mismatch
        let has_type_error = errors.iter().any(|e| {
            e["code"].as_str().unwrap_or("") == "TYPE_MISMATCH"
        });
        assert!(has_type_error, "errors should include TYPE_MISMATCH: {:?}", errors);

        let error = errors.iter().find(|e| e["code"].as_str().unwrap_or("") == "TYPE_MISMATCH").unwrap();
        assert!(error["details"].is_object(), "should have structured details");
        let details = &error["details"];
        assert!(details["source_node"].is_number(), "should have source_node");
        assert!(details["target_node"].is_number(), "should have target_node");
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
    let func_id = add_typed_function(
        &app, pid, "add",
        json!([["a", 3], ["b", 3]]),
        3,
    ).await;

    // Add params (pass validation alone)
    let param_a = insert_param(&app, pid, func_id, 0).await;
    let param_b = insert_param(&app, pid, func_id, 1).await;

    // Batch: add BinaryArith(Add) + Return + all edges
    let add_node_id = param_b + 1; // next available node id
    let ret_node_id = param_b + 2;

    let body = batch_mutate(&app, pid, json!([
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
    ])).await;
    assert!(body["valid"].as_bool().unwrap(), "add function batch should be valid: {:?}", body);
    assert!(body["committed"].as_bool().unwrap(), "should commit: {:?}", body);

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
    assert!(body["success"].as_bool().unwrap(), "simulation should succeed: {:?}", body);

    // Check result = 8 (Value::I32(8) serializes as {"I32": 8})
    let result = &body["result"];
    assert_eq!(result["I32"].as_i64().unwrap(), 8, "3 + 5 should equal 8");

    // Check trace is present when trace_enabled=true
    assert!(body["trace"].is_array(), "trace should be present when enabled");
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
    let content_type = response.headers().get("content-type").unwrap().to_str().unwrap();
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
    let func_id = add_typed_function(
        &app, pid, "mistyped",
        json!([["x", 3], ["y", 6]]),
        3,
    ).await;

    // Add params
    let param_x = insert_param(&app, pid, func_id, 0).await;
    let param_y = insert_param(&app, pid, func_id, 1).await;

    // Batch add BinaryArith + mismatched edges
    let add_node_id = param_y + 1;
    let body = batch_mutate(&app, pid, json!([
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
    ])).await;

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
    let error = errors.iter()
        .find(|e| e["code"].as_str().unwrap_or("") == "TYPE_MISMATCH")
        .expect("should have TYPE_MISMATCH error");

    // Verify structured diagnostic format
    assert!(error["code"].is_string(), "error should have a code field");
    assert!(error["message"].is_string(), "error should have a message field");

    // Per locked decision: no fix suggestions in diagnostics
    assert!(error.get("suggestion").is_none(), "should NOT include fix suggestions");
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
    assert_eq!(overview_mid["node_count"].as_u64().unwrap(), count_before + 1);

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
    assert!(entries.len() >= 3, "expected at least 3 history entries, got {}", entries.len());

    for entry in entries {
        assert!(entry["id"].is_string(), "history entry should have id");
        assert!(entry["timestamp"].is_string(), "history entry should have timestamp");
    }

    // GET checkpoints
    let (status, checkpoints) = get_json(&app, &format!("/programs/{}/checkpoints", pid)).await;
    assert_eq!(status, StatusCode::OK);
    let cp_list = checkpoints["checkpoints"].as_array().unwrap();
    assert!(!cp_list.is_empty(), "should have at least one checkpoint");

    let checkpoint = &cp_list[0];
    assert_eq!(checkpoint["name"].as_str().unwrap(), "mid_point");
    assert!(checkpoint["timestamp"].is_string(), "checkpoint should have timestamp");
    assert!(checkpoint["edit_position"].is_number(), "checkpoint should have edit_position");
}
