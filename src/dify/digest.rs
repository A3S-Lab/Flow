use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::{DifyAppDsl, DifyGraph, DifyImportError};

const EXECUTION_DIGEST_DOMAIN: &[u8] = b"a3s.flow.dify.execution.v1\0";
const GRAPH_EXECUTION_DIGEST_DOMAIN: &[u8] = b"a3s.flow.dify.graph.execution.v1\0";
const NODE_PRESENTATION_FIELDS: &[&str] = &[
    "draggable",
    "height",
    "position",
    "positionAbsolute",
    "selected",
    "selectable",
    "sourcePosition",
    "targetPosition",
    "type",
    "width",
    "zIndex",
];
const NODE_DATA_PRESENTATION_FIELDS: &[&str] = &["desc", "height", "selected", "title", "width"];
const EDGE_PRESENTATION_FIELDS: &[&str] =
    &["animated", "hidden", "selected", "style", "type", "zIndex"];

pub(super) fn document_execution_digest(document: &DifyAppDsl) -> Result<String, DifyImportError> {
    document.graph().execution_plan()?;
    let mut value =
        serde_json::to_value(document).map_err(|error| DifyImportError::Serialization {
            message: error.to_string(),
        })?;
    normalize_document(&mut value)?;
    stable_digest(EXECUTION_DIGEST_DOMAIN, &value)
}

pub(super) fn graph_execution_digest(graph: &DifyGraph) -> Result<String, DifyImportError> {
    graph.execution_plan()?;
    let mut value =
        serde_json::to_value(graph).map_err(|error| DifyImportError::Serialization {
            message: error.to_string(),
        })?;
    normalize_graph(&mut value, "graph")?;
    stable_digest(GRAPH_EXECUTION_DIGEST_DOMAIN, &value)
}

fn normalize_document(value: &mut Value) -> Result<(), DifyImportError> {
    let document = object_mut(value, "document")?;
    let app = document
        .get_mut("app")
        .ok_or_else(|| invalid_serialized("document.app is missing"))?;
    let mode = object_mut(app, "document.app")?
        .remove("mode")
        .ok_or_else(|| invalid_serialized("document.app.mode is missing"))?;
    document.insert(
        "app".to_owned(),
        Value::Object(Map::from_iter([("mode".into(), mode)])),
    );

    let workflow = object_mut(
        document
            .get_mut("workflow")
            .ok_or_else(|| invalid_serialized("document.workflow is missing"))?,
        "document.workflow",
    )?;
    let graph = workflow
        .get_mut("graph")
        .ok_or_else(|| invalid_serialized("document.workflow.graph is missing"))?;
    normalize_graph(graph, "document.workflow.graph")
}

fn normalize_graph(value: &mut Value, label: &str) -> Result<(), DifyImportError> {
    let graph = object_mut(value, label)?;
    graph.remove("viewport");

    let nodes = graph
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_serialized(format!("{label}.nodes is not an array")))?;
    for node in nodes.iter_mut() {
        let node = object_mut(node, "Dify node")?;
        for field in NODE_PRESENTATION_FIELDS {
            node.remove(*field);
        }
        if let Some(data) = node.get_mut("data") {
            let data = object_mut(data, "Dify node.data")?;
            for field in NODE_DATA_PRESENTATION_FIELDS {
                data.remove(*field);
            }
        }
    }
    sort_by_id(nodes, "Dify nodes")?;

    let edges = graph
        .get_mut("edges")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_serialized(format!("{label}.edges is not an array")))?;
    for edge in edges.iter_mut() {
        let edge = object_mut(edge, "Dify edge")?;
        for field in EDGE_PRESENTATION_FIELDS {
            edge.remove(*field);
        }
    }
    sort_by_id(edges, "Dify edges")?;
    Ok(())
}

fn stable_digest(domain: &[u8], value: &Value) -> Result<String, DifyImportError> {
    let encoded = serde_json::to_vec(value).map_err(|error| DifyImportError::Serialization {
        message: error.to_string(),
    })?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(encoded);
    Ok(format!("{:x}", hasher.finalize()))
}

fn sort_by_id(values: &mut [Value], label: &str) -> Result<(), DifyImportError> {
    for value in values.iter() {
        if value.get("id").and_then(Value::as_str).is_none() {
            return Err(invalid_serialized(format!("{label} contain an invalid ID")));
        }
    }
    values.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(right.get("id").and_then(Value::as_str).unwrap_or_default())
    });
    Ok(())
}

fn object_mut<'a>(
    value: &'a mut Value,
    label: &str,
) -> Result<&'a mut Map<String, Value>, DifyImportError> {
    value
        .as_object_mut()
        .ok_or_else(|| invalid_serialized(format!("{label} is not an object")))
}

fn invalid_serialized(message: impl Into<String>) -> DifyImportError {
    DifyImportError::Serialization {
        message: message.into(),
    }
}
