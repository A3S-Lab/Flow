use std::cmp::Ordering;

use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

use super::{WorkflowDag, WorkflowDsl, WorkflowDslError};

const EXECUTION_DIGEST_DOMAIN: &[u8] = b"a3s.flow.workflow_dsl.execution.v2\0";
const GRAPH_EXECUTION_DIGEST_DOMAIN: &[u8] = b"a3s.flow.workflow_dag.execution.v2\0";
const MAX_CANONICAL_DEPTH: usize = 256;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
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
const EDGE_PRESENTATION_FIELDS: &[&str] = &[
    "animated", "hidden", "label", "selected", "style", "type", "zIndex",
];

pub(super) fn document_execution_digest(
    document: &WorkflowDsl,
) -> Result<String, WorkflowDslError> {
    document.graph().execution_plan()?;
    let mut value =
        serde_json::to_value(document).map_err(|error| WorkflowDslError::Serialization {
            message: error.to_string(),
        })?;
    normalize_document(&mut value)?;
    stable_digest(EXECUTION_DIGEST_DOMAIN, &value)
}

pub(super) fn graph_execution_digest(graph: &WorkflowDag) -> Result<String, WorkflowDslError> {
    graph.execution_plan()?;
    let mut value =
        serde_json::to_value(graph).map_err(|error| WorkflowDslError::Serialization {
            message: error.to_string(),
        })?;
    normalize_graph(&mut value, "graph")?;
    stable_digest(GRAPH_EXECUTION_DIGEST_DOMAIN, &value)
}

fn normalize_document(value: &mut Value) -> Result<(), WorkflowDslError> {
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

fn normalize_graph(value: &mut Value, label: &str) -> Result<(), WorkflowDslError> {
    let graph = object_mut(value, label)?;
    graph.remove("viewport");

    let nodes = graph
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_serialized(format!("{label}.nodes is not an array")))?;
    for node in nodes.iter_mut() {
        let node = object_mut(node, "workflow DAG node")?;
        for field in NODE_PRESENTATION_FIELDS {
            node.remove(*field);
        }
        if let Some(data) = node.get_mut("data") {
            let data = object_mut(data, "workflow DAG node.data")?;
            for field in NODE_DATA_PRESENTATION_FIELDS {
                data.remove(*field);
            }
        }
    }
    sort_by_id(nodes, "workflow DAG nodes")?;

    let edges = graph
        .get_mut("edges")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_serialized(format!("{label}.edges is not an array")))?;
    for edge in edges.iter_mut() {
        let edge = object_mut(edge, "workflow DAG edge")?;
        for field in EDGE_PRESENTATION_FIELDS {
            edge.remove(*field);
        }
    }
    sort_by_id(edges, "workflow DAG edges")?;
    Ok(())
}

fn stable_digest(domain: &[u8], value: &Value) -> Result<String, WorkflowDslError> {
    let encoded = canonical_json(value)?;
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(encoded.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

/// Encode a JSON value using the canonical form shared with the TypeScript
/// Flow UI.  The format intentionally follows JSON.stringify semantics for
/// numbers and UTF-16 key ordering so semantically identical documents have
/// one cross-language identity.
pub(super) fn canonical_json(value: &Value) -> Result<String, WorkflowDslError> {
    let mut encoded = String::new();
    write_canonical_json(value, &mut encoded, 0)?;
    Ok(encoded)
}

fn write_canonical_json(
    value: &Value,
    encoded: &mut String,
    depth: usize,
) -> Result<(), WorkflowDslError> {
    if depth > MAX_CANONICAL_DEPTH {
        return Err(serialization_error(format!(
            "canonical JSON exceeds maximum depth {MAX_CANONICAL_DEPTH}"
        )));
    }

    match value {
        Value::Null => encoded.push_str("null"),
        Value::Bool(value) => encoded.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => write_canonical_number(number, encoded)?,
        Value::String(value) => {
            let escaped = serde_json::to_string(value)
                .map_err(|error| serialization_error(error.to_string()))?;
            encoded.push_str(&escaped);
        }
        Value::Array(values) => {
            encoded.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    encoded.push(',');
                }
                write_canonical_json(value, encoded, depth + 1)?;
            }
            encoded.push(']');
        }
        Value::Object(values) => write_canonical_object(values, encoded, depth)?,
    }
    Ok(())
}

fn write_canonical_object(
    values: &Map<String, Value>,
    encoded: &mut String,
    depth: usize,
) -> Result<(), WorkflowDslError> {
    let mut keys: Vec<&str> = values.keys().map(String::as_str).collect();
    // JavaScript's Array.prototype.sort compares UTF-16 code units. Rust's
    // native Unicode ordering is not equivalent for non-BMP identifiers.
    keys.sort_by(|left, right| compare_utf16(left, right));

    encoded.push('{');
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            encoded.push(',');
        }
        let escaped_key =
            serde_json::to_string(key).map_err(|error| serialization_error(error.to_string()))?;
        encoded.push_str(&escaped_key);
        encoded.push(':');
        let value = values.get(*key).ok_or_else(|| {
            serialization_error(format!(
                "canonical object key {key:?} disappeared during serialization"
            ))
        })?;
        write_canonical_json(value, encoded, depth + 1)?;
    }
    encoded.push('}');
    Ok(())
}

fn compare_utf16(left: &str, right: &str) -> Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn write_canonical_number(number: &Number, encoded: &mut String) -> Result<(), WorkflowDslError> {
    if let Some(value) = number.as_i64() {
        if value.unsigned_abs() > MAX_SAFE_INTEGER {
            return Err(unsafe_integer_error(value.to_string()));
        }
        let mut buffer = ryu_js::Buffer::new();
        encoded.push_str(buffer.format(value as f64));
        return Ok(());
    }
    if let Some(value) = number.as_u64() {
        if value > MAX_SAFE_INTEGER {
            return Err(unsafe_integer_error(value.to_string()));
        }
        let mut buffer = ryu_js::Buffer::new();
        encoded.push_str(buffer.format(value as f64));
        return Ok(());
    }

    let value = number
        .as_f64()
        .ok_or_else(|| serialization_error(format!("unsupported JSON number {number}")))?;
    if !value.is_finite() {
        return Err(serialization_error(
            "canonical JSON does not support non-finite numbers",
        ));
    }
    if value.fract() == 0.0 && value.abs() > MAX_SAFE_INTEGER as f64 {
        return Err(unsafe_integer_error(value.to_string()));
    }
    let normalized = if value == 0.0 { 0.0 } else { value };
    let mut buffer = ryu_js::Buffer::new();
    encoded.push_str(buffer.format(normalized));
    Ok(())
}

fn unsafe_integer_error(value: String) -> WorkflowDslError {
    serialization_error(format!(
        "JSON integer {value} exceeds JavaScript safe integer range"
    ))
}

fn serialization_error(message: impl Into<String>) -> WorkflowDslError {
    WorkflowDslError::Serialization {
        message: message.into(),
    }
}

fn sort_by_id(values: &mut [Value], label: &str) -> Result<(), WorkflowDslError> {
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
) -> Result<&'a mut Map<String, Value>, WorkflowDslError> {
    value
        .as_object_mut()
        .ok_or_else(|| invalid_serialized(format!("{label} is not an object")))
}

fn invalid_serialized(message: impl Into<String>) -> WorkflowDslError {
    WorkflowDslError::Serialization {
        message: message.into(),
    }
}
