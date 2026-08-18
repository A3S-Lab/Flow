use a3s_flow::{DifyAppDsl, DifyDslCompatibility, DifyGraph, DifyImportError};
use serde_json::json;

const DIFY_ECHO: &str = include_str!("fixtures/dify_echo.yml");

#[test]
fn complete_dify_yaml_round_trips_without_losing_vendor_fields() {
    let document = DifyAppDsl::from_yaml(DIFY_ECHO).expect("import current Dify DSL");

    assert_eq!(document.version(), "0.7.0");
    assert_eq!(document.app().name(), "A3S Dify import fixture");
    assert_eq!(document.graph().nodes().len(), 3);
    assert_eq!(document.graph().edges().len(), 2);
    assert_eq!(
        document.graph().node("llm").expect("LLM node").data()["x-a3s-fixture-extension"],
        json!({"retained": true})
    );
    assert_eq!(
        document.extensions()["x-a3s-document-extension"],
        json!({"retained": true})
    );

    let encoded = document.to_yaml().expect("export imported Dify DSL");
    let reparsed = DifyAppDsl::from_yaml(&encoded).expect("re-import exported Dify DSL");
    assert_eq!(reparsed, document);
}

#[test]
fn extracted_graph_json_uses_the_native_dify_nodes_and_edges_shape() {
    let graph = DifyGraph::from_json(
        &json!({
            "nodes": [
                {"id": "start", "data": {"type": "start"}, "position": {"x": 0, "y": 0}},
                {"id": "end", "data": {"type": "end"}, "position": {"x": 300, "y": 0}}
            ],
            "edges": [
                {
                    "id": "start-source-end-target",
                    "source": "start",
                    "sourceHandle": "source",
                    "target": "end",
                    "targetHandle": "target",
                    "data": {"sourceType": "start", "targetType": "end"}
                }
            ],
            "viewport": {"x": 0, "y": 0, "zoom": 1}
        })
        .to_string(),
    )
    .expect("import extracted Dify graph");

    assert_eq!(
        graph.node("start").expect("start node").node_type(),
        "start"
    );
    assert_eq!(graph.node("end").expect("end node").node_type(), "end");
    assert_eq!(
        graph
            .execution_plan()
            .expect("valid executable graph")
            .top_level(),
        ["start", "end"]
    );
}

#[test]
fn empty_dify_canvas_imports_as_a_draft_but_is_not_executable() {
    let graph = DifyGraph::from_json(r#"{"nodes":[],"edges":[]}"#)
        .expect("empty Dify drafts are importable");

    assert!(matches!(
        graph.execution_plan(),
        Err(DifyImportError::InvalidGraph { .. })
    ));
}

#[test]
fn execution_validation_rejects_duplicate_nodes_dangling_edges_and_cycles() {
    for (graph, expected) in [
        (
            json!({
                "nodes": [
                    {"id": "same", "data": {"type": "start"}},
                    {"id": "same", "data": {"type": "end"}}
                ],
                "edges": []
            }),
            "duplicate node ID",
        ),
        (
            json!({
                "nodes": [{"id": "start", "data": {"type": "start"}}],
                "edges": [{"id": "missing", "source": "start", "target": "missing"}]
            }),
            "missing target",
        ),
        (
            json!({
                "nodes": [
                    {"id": "a", "data": {"type": "start"}},
                    {"id": "b", "data": {"type": "end"}}
                ],
                "edges": [
                    {"id": "a-b", "source": "a", "target": "b"},
                    {"id": "b-a", "source": "b", "target": "a"}
                ]
            }),
            "cycle",
        ),
    ] {
        let error = DifyGraph::from_json(&graph.to_string())
            .expect("structural import")
            .execution_plan()
            .expect_err("invalid graph must not compile");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn execution_digest_ignores_canvas_layout_but_binds_node_configuration() {
    let original = DifyAppDsl::from_yaml(DIFY_ECHO).expect("fixture");
    let mut layout_change: serde_json::Value =
        serde_yaml_ng::from_str(DIFY_ECHO).expect("fixture YAML value");
    layout_change["workflow"]["graph"]["nodes"][1]["position"] = json!({"x": 999, "y": -123});
    layout_change["workflow"]["graph"]["viewport"] = json!({"x": 42, "y": 42, "zoom": 0.25});
    let layout_change =
        DifyAppDsl::from_yaml(&serde_yaml_ng::to_string(&layout_change).expect("layout YAML"))
            .expect("layout-only import");
    assert_eq!(
        original.execution_digest().expect("original digest"),
        layout_change.execution_digest().expect("layout digest")
    );

    let mut semantic_change: serde_json::Value =
        serde_yaml_ng::from_str(DIFY_ECHO).expect("fixture YAML value");
    semantic_change["workflow"]["graph"]["nodes"][1]["data"]["model"]["name"] =
        json!("different-model");
    let semantic_change =
        DifyAppDsl::from_yaml(&serde_yaml_ng::to_string(&semantic_change).expect("semantic YAML"))
            .expect("semantic import");
    assert_ne!(
        original.execution_digest().expect("original digest"),
        semantic_change.execution_digest().expect("semantic digest")
    );
}

#[test]
fn extracted_graph_digest_is_order_and_layout_independent() {
    let first = DifyGraph::from_json(
        &json!({
            "nodes": [
                {"id": "start", "position": {"x": 0, "y": 0}, "data": {"type": "start"}},
                {"id": "end", "position": {"x": 10, "y": 10}, "data": {"type": "end", "outputs": []}}
            ],
            "edges": [{"id": "edge", "source": "start", "target": "end"}],
            "viewport": {"x": 0, "y": 0, "zoom": 1}
        })
        .to_string(),
    )
    .expect("first graph");
    let reordered = DifyGraph::from_json(
        &json!({
            "nodes": [
                {"id": "end", "position": {"x": 999, "y": 999}, "data": {"type": "end", "outputs": []}},
                {"id": "start", "position": {"x": -10, "y": -10}, "data": {"type": "start"}}
            ],
            "edges": [{"id": "edge", "source": "start", "target": "end"}],
            "viewport": {"x": 20, "y": 30, "zoom": 0.2}
        })
        .to_string(),
    )
    .expect("reordered graph");

    assert_eq!(
        first.execution_digest().expect("first digest"),
        reordered.execution_digest().expect("reordered digest")
    );
}

#[test]
fn dify_dsl_version_compatibility_matches_the_upstream_import_contract() {
    let document = DifyAppDsl::from_yaml(DIFY_ECHO).expect("current fixture");
    assert_eq!(
        document.compatibility().expect("current version"),
        DifyDslCompatibility::Compatible
    );

    for (version, expected) in [
        ("0.6.9", DifyDslCompatibility::CompatibleWithWarnings),
        ("0.7.1", DifyDslCompatibility::RequiresConfirmation),
        ("1.0.0", DifyDslCompatibility::RequiresConfirmation),
    ] {
        let source = DIFY_ECHO.replacen("version: 0.7.0", &format!("version: {version}"), 1);
        let imported = DifyAppDsl::from_yaml(&source).expect("well-formed Dify version");
        assert_eq!(imported.compatibility().expect("compatibility"), expected);
    }

    let invalid = DIFY_ECHO.replacen("version: 0.7.0", "version: latest", 1);
    assert!(matches!(
        DifyAppDsl::from_yaml(&invalid),
        Err(DifyImportError::InvalidDocument { .. })
    ));
}

#[test]
fn nested_dify_iteration_compiles_each_react_flow_scope_deterministically() {
    let graph = DifyGraph::from_json(
        &json!({
            "nodes": [
                {"id": "end", "data": {"type": "end"}},
                {
                    "id": "iteration",
                    "data": {"type": "iteration", "start_node_id": "iteration-start"}
                },
                {"id": "start", "data": {"type": "start"}},
                {
                    "id": "inner",
                    "parentId": "iteration",
                    "data": {"type": "template-transform", "iteration_id": "iteration"}
                },
                {
                    "id": "iteration-start",
                    "parentId": "iteration",
                    "data": {"type": "iteration-start"}
                }
            ],
            "edges": [
                {"id": "iteration-end", "source": "iteration", "target": "end"},
                {"id": "start-iteration", "source": "start", "target": "iteration"},
                {
                    "id": "iteration-start-inner",
                    "source": "iteration-start",
                    "target": "inner",
                    "data": {"isInIteration": true, "iteration_id": "iteration"}
                }
            ]
        })
        .to_string(),
    )
    .expect("nested Dify graph");

    let plan = graph.execution_plan().expect("valid nested plan");
    assert_eq!(plan.top_level(), ["start", "iteration", "end"]);
    assert_eq!(
        plan.scope("iteration").expect("iteration scope"),
        ["iteration-start", "inner"]
    );
}

#[test]
fn executable_dify_graph_rejects_invalid_container_references() {
    for (graph, expected) in [
        (
            json!({
                "nodes": [
                    {"id": "start", "data": {"type": "start"}},
                    {"id": "child", "parentId": "missing", "data": {"type": "code"}}
                ],
                "edges": []
            }),
            "missing parent",
        ),
        (
            json!({
                "nodes": [
                    {"id": "start", "data": {"type": "start"}},
                    {"id": "child", "parentId": "start", "data": {"type": "code"}}
                ],
                "edges": []
            }),
            "is not an iteration or loop",
        ),
        (
            json!({
                "nodes": [
                    {"id": "iteration", "data": {"type": "iteration", "start_node_id": "inner-start"}},
                    {"id": "inner-start", "parentId": "iteration", "data": {"type": "loop-start"}}
                ],
                "edges": []
            }),
            "requires an iteration-start",
        ),
    ] {
        let error = DifyGraph::from_json(&graph.to_string())
            .expect("structural import")
            .execution_plan()
            .expect_err("invalid container graph must not compile");
        assert!(error.to_string().contains(expected), "{error}");
    }
}
