use a3s_flow::{
    WorkflowDag, WorkflowDagEdge, WorkflowDagNode, WorkflowDsl, WorkflowDslCompatibility,
    WorkflowDslError,
};
use serde::Deserialize;
use serde_json::json;

const WORKFLOW_DSL_ECHO: &str = include_str!("fixtures/workflow_dsl_echo.yml");
const WORKFLOW_DIGEST_VECTORS: &str =
    include_str!("../packages/ui/tests/fixtures/workflow-digest-vectors.json");

#[derive(Debug, Deserialize)]
struct WorkflowDigestVector {
    name: String,
    graph: serde_json::Value,
    document: serde_json::Value,
    #[serde(rename = "graphDigest")]
    graph_digest: String,
    #[serde(rename = "documentDigest")]
    document_digest: String,
}

#[test]
fn complete_workflow_yaml_round_trips_without_losing_vendor_fields() {
    let document = WorkflowDsl::from_yaml(WORKFLOW_DSL_ECHO).expect("import current workflow DSL");

    assert_eq!(document.version(), "0.7.0");
    assert_eq!(document.app().name(), "A3S workflow import fixture");
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

    let encoded = document.to_yaml().expect("export imported workflow DSL");
    let reparsed = WorkflowDsl::from_yaml(&encoded).expect("re-import exported workflow DSL");
    assert_eq!(reparsed, document);
}

#[test]
fn workflow_dsl_rejects_non_workflow_application_modes() {
    let source = WORKFLOW_DSL_ECHO.replacen("mode: workflow", "mode: advanced-chat", 1);

    assert!(matches!(
        WorkflowDsl::from_yaml(&source),
        Err(WorkflowDslError::InvalidDocument { .. })
    ));
}

#[test]
fn extracted_graph_json_uses_the_native_nodes_and_edges_shape() {
    let graph = WorkflowDag::from_json(
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
    .expect("import extracted workflow DAG");

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
fn empty_workflow_canvas_imports_as_a_draft_but_is_not_executable() {
    let graph = WorkflowDag::from_json(r#"{"nodes":[],"edges":[]}"#)
        .expect("empty workflow drafts are importable");

    assert!(matches!(
        graph.execution_plan(),
        Err(WorkflowDslError::InvalidGraph { .. })
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
        let error = WorkflowDag::from_json(&graph.to_string())
            .expect("structural import")
            .execution_plan()
            .expect_err("invalid graph must not compile");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn execution_digest_ignores_canvas_layout_but_binds_node_configuration() {
    let original = WorkflowDsl::from_yaml(WORKFLOW_DSL_ECHO).expect("fixture");
    let mut layout_change: serde_json::Value =
        serde_yaml_ng::from_str(WORKFLOW_DSL_ECHO).expect("fixture YAML value");
    layout_change["workflow"]["graph"]["nodes"][1]["position"] = json!({"x": 999, "y": -123});
    layout_change["workflow"]["graph"]["viewport"] = json!({"x": 42, "y": 42, "zoom": 0.25});
    let layout_change =
        WorkflowDsl::from_yaml(&serde_yaml_ng::to_string(&layout_change).expect("layout YAML"))
            .expect("layout-only import");
    assert_eq!(
        original.execution_digest().expect("original digest"),
        layout_change.execution_digest().expect("layout digest")
    );

    let mut semantic_change: serde_json::Value =
        serde_yaml_ng::from_str(WORKFLOW_DSL_ECHO).expect("fixture YAML value");
    semantic_change["workflow"]["graph"]["nodes"][1]["data"]["model"]["name"] =
        json!("different-model");
    let semantic_change =
        WorkflowDsl::from_yaml(&serde_yaml_ng::to_string(&semantic_change).expect("semantic YAML"))
            .expect("semantic import");
    assert_ne!(
        original.execution_digest().expect("original digest"),
        semantic_change.execution_digest().expect("semantic digest")
    );
}

#[test]
fn extracted_graph_digest_is_order_and_layout_independent() {
    let first = WorkflowDag::from_json(
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
    let reordered = WorkflowDag::from_json(
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
fn custom_host_node_round_trips_compiles_and_binds_its_configuration() {
    let source = json!({
        "nodes": [
            {
                "id": "start",
                "position": {"x": 0, "y": 0},
                "data": {"type": "flow.start"}
            },
            {
                "id": "risk",
                "position": {"x": 320, "y": 0},
                "data": {
                    "type": "commerce.risk.score",
                    "policy": "strict-v1",
                    "review_threshold": 0.78,
                    "strict_validation": true,
                    "parameters": {"market": "cross-border"},
                    "x-host-capability": {
                        "id": "commerce/risk-score",
                        "version": "1.2.3",
                        "handler": "risk.score-order"
                    }
                }
            },
            {
                "id": "complete",
                "position": {"x": 640, "y": 0},
                "data": {"type": "flow.complete"}
            }
        ],
        "edges": [
            {"id": "start-risk", "source": "start", "target": "risk"},
            {"id": "risk-complete", "source": "risk", "target": "complete"}
        ]
    });
    let graph = WorkflowDag::from_json(&source.to_string()).expect("custom-node graph");

    assert_eq!(
        graph
            .execution_plan()
            .expect("custom node participates in the deterministic plan")
            .top_level(),
        ["start", "risk", "complete"]
    );
    let risk = graph.node("risk").expect("custom risk node");
    assert_eq!(risk.node_type(), "commerce.risk.score");
    assert_eq!(risk.data()["review_threshold"], json!(0.78));
    assert_eq!(
        risk.data()["x-host-capability"]["handler"],
        json!("risk.score-order")
    );

    let encoded = graph.to_json().expect("serialize custom-node graph");
    assert_eq!(
        WorkflowDag::from_json(&encoded).expect("re-import custom-node graph"),
        graph
    );

    let original_digest = graph.execution_digest().expect("custom-node digest");
    let mut configuration_change = source.clone();
    configuration_change["nodes"][1]["data"]["review_threshold"] = json!(0.91);
    let configuration_change =
        WorkflowDag::from_json(&configuration_change.to_string()).expect("configuration change");
    assert_ne!(
        configuration_change
            .execution_digest()
            .expect("changed custom-node digest"),
        original_digest,
        "custom-node configuration is part of executable semantics"
    );

    let mut layout_change = source;
    layout_change["nodes"][1]["position"] = json!({"x": 999, "y": -400});
    let layout_change =
        WorkflowDag::from_json(&layout_change.to_string()).expect("layout-only change");
    assert_eq!(
        layout_change
            .execution_digest()
            .expect("layout-only custom-node digest"),
        original_digest,
        "custom-node canvas layout is not executable semantics"
    );
}

#[test]
fn execution_digest_matches_the_cross_language_golden_vectors() {
    let vectors: Vec<WorkflowDigestVector> =
        serde_json::from_str(WORKFLOW_DIGEST_VECTORS).expect("digest vectors");

    for vector in vectors {
        let graph = WorkflowDag::from_json(&vector.graph.to_string())
            .unwrap_or_else(|error| panic!("{} graph: {error}", vector.name));
        let document = WorkflowDsl::from_json(&vector.document.to_string())
            .unwrap_or_else(|error| panic!("{} document: {error}", vector.name));

        assert_eq!(
            graph.execution_digest().expect("graph digest"),
            vector.graph_digest,
            "graph vector {}",
            vector.name
        );
        assert_eq!(
            document.execution_digest().expect("document digest"),
            vector.document_digest,
            "document vector {}",
            vector.name
        );
    }
}

#[test]
fn execution_digest_rejects_numbers_that_javascript_cannot_represent_safely() {
    let graph = WorkflowDag::from_json(
        &json!({
            "nodes": [
                {"id": "start", "data": {"type": "start", "unsafe": 9007199254740992u64}},
                {"id": "end", "data": {"type": "end"}}
            ],
            "edges": [{"id": "start-end", "source": "start", "target": "end"}]
        })
        .to_string(),
    )
    .expect("unsafe integer graph is structurally importable");

    let error = graph
        .execution_digest()
        .expect_err("unsafe integer must not receive a cross-language digest");
    assert!(error.to_string().contains("safe integer"), "{error}");
}

#[test]
fn workflow_dsl_version_compatibility_matches_the_wire_contract() {
    let document = WorkflowDsl::from_yaml(WORKFLOW_DSL_ECHO).expect("current fixture");
    assert_eq!(
        document.compatibility().expect("current version"),
        WorkflowDslCompatibility::Compatible
    );

    for (version, expected) in [
        ("0.6.9", WorkflowDslCompatibility::CompatibleWithWarnings),
        ("0.7.1", WorkflowDslCompatibility::RequiresConfirmation),
        ("1.0.0", WorkflowDslCompatibility::RequiresConfirmation),
    ] {
        let source =
            WORKFLOW_DSL_ECHO.replacen("version: 0.7.0", &format!("version: {version}"), 1);
        let imported = WorkflowDsl::from_yaml(&source).expect("well-formed workflow DSL version");
        assert_eq!(imported.compatibility().expect("compatibility"), expected);
    }

    let invalid = WORKFLOW_DSL_ECHO.replacen("version: 0.7.0", "version: latest", 1);
    assert!(matches!(
        WorkflowDsl::from_yaml(&invalid),
        Err(WorkflowDslError::InvalidDocument { .. })
    ));
}

#[test]
fn nested_iteration_compiles_each_canvas_scope_deterministically() {
    let graph = WorkflowDag::from_json(
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
    .expect("nested workflow DAG");

    let plan = graph.execution_plan().expect("valid nested plan");
    assert_eq!(plan.top_level(), ["start", "iteration", "end"]);
    assert_eq!(
        plan.scope("iteration").expect("iteration scope"),
        ["iteration-start", "inner"]
    );
}

#[test]
fn executable_workflow_dag_rejects_invalid_container_references() {
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
        let error = WorkflowDag::from_json(&graph.to_string())
            .expect("structural import")
            .execution_plan()
            .expect_err("invalid container graph must not compile");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn programmatic_dag_construction_uses_the_same_structural_compiler() {
    let graph = WorkflowDag::new(
        vec![
            WorkflowDagNode::new("output", "output"),
            WorkflowDagNode::new("input", "input"),
        ],
        vec![WorkflowDagEdge::new("input-output", "input", "output").with_source_handle("success")],
    );

    assert_eq!(
        graph
            .execution_plan()
            .expect("programmatic DAG")
            .top_level(),
        ["input", "output"]
    );
    assert_eq!(
        graph.edges()[0].source_handle(),
        Some("success"),
        "edge metadata stays available to the product compiler"
    );
}
