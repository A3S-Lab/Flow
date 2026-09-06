//! Stateless workflow-document authoring primitives.
//!
//! This module is the Rust counterpart of the Flow UI authoring operations. It
//! owns parsing and applying bounded JSON operations, including incremental
//! sessions, while hosts remain responsible for authorization, persistence,
//! and product-specific node catalogs. The functions return canonical JSON
//! bytes so a host can persist a single representation and derive its own
//! storage digest.

use serde_json::{Map, Value};
use std::collections::BTreeSet;

use super::{WorkflowDsl, WorkflowDslError, WORKFLOW_DSL_MAX_BYTES};

/// Maximum encoded size accepted for one authoring operation.
pub const WORKFLOW_AUTHORING_OPERATION_MAX_BYTES: usize = 1024 * 1024;
/// Maximum byte length of one authoring identity field.
pub const WORKFLOW_AUTHORING_ID_MAX_BYTES: usize = 255;
/// Maximum number of operations accepted by one authoring session or batch.
pub const WORKFLOW_AUTHORING_MAX_OPERATIONS: usize = 10_000;

/// Incremental authoring session for one immutable workflow snapshot.
///
/// A session parses the base document once and applies bounded operations as
/// they arrive. The input snapshot is never modified, and the session only
/// emits a canonical snapshot when [`Self::finish`] or
/// [`Self::finish_executable`] is called. Hosts can therefore consume NDJSON
/// or another transport incrementally without buffering the operation list.
/// The caller remains responsible for durable publication and concurrency
/// control around the returned bytes.
#[derive(Debug)]
pub struct WorkflowAuthoringSession {
    document: Value,
    operation_count: usize,
}

impl WorkflowAuthoringSession {
    /// Start a session from a bounded, structurally valid workflow snapshot.
    pub fn new(base_snapshot: &[u8]) -> Result<Self, WorkflowDslError> {
        let document = parse_snapshot(base_snapshot)?;
        let document = serde_json::to_value(&document).map_err(serialization_error)?;
        Ok(Self {
            document,
            operation_count: 0,
        })
    }

    /// Apply one operation to the working snapshot.
    ///
    /// Parsing and all operation preconditions complete before the working
    /// value is changed. A rejected operation therefore leaves the session at
    /// the last successful state and can be reported without publishing it.
    pub fn apply_operation(&mut self, operation: &[u8]) -> Result<(), WorkflowDslError> {
        if self.operation_count >= WORKFLOW_AUTHORING_MAX_OPERATIONS {
            return Err(invalid_operation(format!(
                "operation count exceeds {WORKFLOW_AUTHORING_MAX_OPERATIONS}"
            )));
        }
        if operation.is_empty() || operation.len() > WORKFLOW_AUTHORING_OPERATION_MAX_BYTES {
            return Err(invalid_operation(format!(
                "operation bytes must be between 1 and {} bytes",
                WORKFLOW_AUTHORING_OPERATION_MAX_BYTES
            )));
        }
        let operation = parse_operation(operation)?;
        apply_operation_value(&mut self.document, &operation)?;
        self.operation_count += 1;
        Ok(())
    }

    /// Return the number of successfully applied operations.
    pub fn operation_count(&self) -> usize {
        self.operation_count
    }

    /// Finish the session as a canonical draft-capable snapshot.
    pub fn finish(self) -> Result<Vec<u8>, WorkflowDslError> {
        canonical_document_value(self.document)
    }

    /// Finish the session and require an executable deterministic DAG.
    pub fn finish_executable(self) -> Result<Vec<u8>, WorkflowDslError> {
        let canonical = canonical_document_value(self.document)?;
        validate_executable_workflow_authoring_snapshot(&canonical)
    }
}

/// Canonicalize and document-validate a workflow authoring snapshot.
///
/// Empty graphs are accepted because they are useful drafts. Call
/// [`validate_executable_workflow_authoring_snapshot`] at publication or run
/// admission when a complete executable DAG is required.
pub fn canonical_workflow_authoring_snapshot(source: &[u8]) -> Result<Vec<u8>, WorkflowDslError> {
    let document = parse_snapshot(source)?;
    canonical_document(&document)
}

/// Parse one authoring operation and encode it in the canonical JSON form.
///
/// The canonical bytes are stable across Rust and TypeScript for equivalent
/// operations (object-key order and insignificant whitespace do not affect
/// them). Defaults that have one wire-level meaning are materialized, while
/// omitted optional fields retain their omission. Hosts should use these
/// bytes when deriving an operation idempotency key or appending an operation
/// to a hosted journal. Node existence and graph placement are intentionally
/// not checked here; those preconditions require a base snapshot and are
/// enforced by [`apply_workflow_authoring_operation`].
pub fn canonical_workflow_authoring_operation(source: &[u8]) -> Result<Vec<u8>, WorkflowDslError> {
    if source.is_empty() || source.len() > WORKFLOW_AUTHORING_OPERATION_MAX_BYTES {
        return Err(invalid_operation(format!(
            "operation bytes must be between 1 and {} bytes",
            WORKFLOW_AUTHORING_OPERATION_MAX_BYTES
        )));
    }
    let operation = parse_operation(source)?;
    let operation = normalize_operation(operation)?;
    let encoded = super::digest::canonical_json(&Value::Object(operation))
        .map_err(|error| invalid_operation(format!("operation cannot be canonicalized: {error}")))?
        .into_bytes();
    if encoded.len() > WORKFLOW_AUTHORING_OPERATION_MAX_BYTES {
        return Err(invalid_operation(format!(
            "canonical operation exceeds {WORKFLOW_AUTHORING_OPERATION_MAX_BYTES} bytes"
        )));
    }
    Ok(encoded)
}

/// Canonicalize a snapshot and require an executable deterministic DAG.
pub fn validate_executable_workflow_authoring_snapshot(
    source: &[u8],
) -> Result<Vec<u8>, WorkflowDslError> {
    let document = parse_snapshot(source)?;
    document.execution_digest()?;
    canonical_document(&document)
}

/// Apply one bounded JSON authoring operation and return a canonical snapshot.
///
/// The source is never modified in place. A failed operation therefore cannot
/// publish a partial document. Intermediate draft states (for example, a
/// container before its start node is added) are allowed; executable admission
/// is a separate explicit operation.
pub fn apply_workflow_authoring_operation(
    base_snapshot: &[u8],
    operation: &[u8],
) -> Result<Vec<u8>, WorkflowDslError> {
    let mut session = WorkflowAuthoringSession::new(base_snapshot)?;
    session.apply_operation(operation)?;
    session.finish()
}

/// Apply a bounded operation iterator without buffering the complete list.
///
/// The iterator may yield borrowed slices, owned byte vectors, or any other
/// item implementing [`AsRef<[u8]>`]. An empty iterator is rejected so a host
/// cannot mistake a no-op stream for a successful publication.
pub fn apply_workflow_authoring_operations<I, O>(
    base_snapshot: &[u8],
    operations: I,
) -> Result<Vec<u8>, WorkflowDslError>
where
    I: IntoIterator<Item = O>,
    O: AsRef<[u8]>,
{
    let mut session = WorkflowAuthoringSession::new(base_snapshot)?;
    let mut applied = 0;
    for operation in operations {
        session.apply_operation(operation.as_ref())?;
        applied += 1;
    }
    if applied == 0 {
        return Err(invalid_operation(
            "authoring operation stream must contain at least one operation",
        ));
    }
    session.finish()
}

fn parse_snapshot(source: &[u8]) -> Result<WorkflowDsl, WorkflowDslError> {
    if source.is_empty() || source.len() > WORKFLOW_DSL_MAX_BYTES {
        return Err(WorkflowDslError::DocumentTooLarge {
            actual_bytes: source.len(),
            maximum_bytes: WORKFLOW_DSL_MAX_BYTES,
        });
    }
    let source = std::str::from_utf8(source).map_err(|error| WorkflowDslError::InvalidJson {
        message: format!("workflow authoring snapshot is not valid UTF-8: {error}"),
    })?;
    WorkflowDsl::from_json(source)
}

fn canonical_document(document: &WorkflowDsl) -> Result<Vec<u8>, WorkflowDslError> {
    let encoded = document.to_json()?.into_bytes();
    if encoded.len() > WORKFLOW_DSL_MAX_BYTES {
        return Err(WorkflowDslError::DocumentTooLarge {
            actual_bytes: encoded.len(),
            maximum_bytes: WORKFLOW_DSL_MAX_BYTES,
        });
    }
    Ok(encoded)
}

fn canonical_document_value(document: Value) -> Result<Vec<u8>, WorkflowDslError> {
    let encoded = serde_json::to_vec(&document).map_err(serialization_error)?;
    canonical_workflow_authoring_snapshot(&encoded)
}

fn parse_operation(source: &[u8]) -> Result<Map<String, Value>, WorkflowDslError> {
    let value: Value = super::strict_json::from_slice(source)
        .map_err(|error| invalid_operation(format!("operation is not valid JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid_operation("operation must be a JSON object"))?;
    let kind = required_string(object, "kind")?;
    let allowed: &[&str] = match kind {
        "add-node" => &["kind", "id", "type", "configuration", "parentId"],
        "move-node" => &["kind", "id", "parentId"],
        "remove-node" => &["kind", "id"],
        "add-edge" => &[
            "kind",
            "id",
            "source",
            "target",
            "sourceHandle",
            "targetHandle",
        ],
        "remove-edge" => &["kind", "id"],
        "set-edge" => &[
            "kind",
            "id",
            "source",
            "target",
            "sourceHandle",
            "targetHandle",
        ],
        "set-node" => &["kind", "id", "configuration"],
        "set-app-name" => &["kind", "name"],
        _ => {
            return Err(invalid_operation(format!(
                "unsupported operation kind {kind:?}"
            )))
        }
    };
    if let Some(unexpected) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid_operation(format!(
            "operation kind {kind:?} has unknown property {unexpected:?}"
        )));
    }
    Ok(object.clone())
}

/// Validate operation-local fields and materialize defaults whose omission is
/// semantically equivalent to an explicit value. This is deliberately kept
/// separate from graph-dependent application so canonicalization can be used
/// before a host has loaded or locked its base snapshot.
fn normalize_operation(
    mut operation: Map<String, Value>,
) -> Result<Map<String, Value>, WorkflowDslError> {
    let kind = required_string(&operation, "kind")?;
    match kind {
        "add-node" => {
            bounded_string(&operation, "id")?;
            bounded_string(&operation, "type")?;
            optional_non_null_string(&operation, "parentId")?;
            if !operation.contains_key("configuration") {
                operation.insert("configuration".to_owned(), Value::Object(Map::new()));
            } else {
                required_object(&operation, "configuration")?;
            }
        }
        "move-node" => {
            bounded_string(&operation, "id")?;
            // An omitted parent and an explicit null both mean top-level.
            if !operation.contains_key("parentId") {
                operation.insert("parentId".to_owned(), Value::Null);
            } else if operation
                .get("parentId")
                .is_some_and(|parent_id| !parent_id.is_null())
            {
                bounded_string(&operation, "parentId")?;
            }
        }
        "remove-node" => {
            bounded_string(&operation, "id")?;
        }
        "add-edge" => {
            bounded_string(&operation, "id")?;
            bounded_string(&operation, "source")?;
            bounded_string(&operation, "target")?;
            optional_non_null_string(&operation, "sourceHandle")?;
            optional_non_null_string(&operation, "targetHandle")?;
        }
        "remove-edge" => {
            bounded_string(&operation, "id")?;
        }
        "set-edge" => {
            bounded_string(&operation, "id")?;
            bounded_string(&operation, "source")?;
            bounded_string(&operation, "target")?;
            optional_nullable_string(&operation, "sourceHandle")?;
            optional_nullable_string(&operation, "targetHandle")?;
        }
        "set-node" => {
            bounded_string(&operation, "id")?;
            required_object(&operation, "configuration")?;
        }
        "set-app-name" => {
            required_string(&operation, "name")?;
        }
        _ => {
            return Err(invalid_operation(format!(
                "unsupported operation kind {kind:?}"
            )))
        }
    }
    Ok(operation)
}

fn apply_operation_value(
    document: &mut Value,
    operation: &Map<String, Value>,
) -> Result<(), WorkflowDslError> {
    let kind = required_string(operation, "kind")?;
    match kind {
        "add-node" => add_node(document, operation),
        "move-node" => move_node(document, operation),
        "remove-node" => remove_node(document, operation),
        "add-edge" => add_edge(document, operation),
        "remove-edge" => remove_edge(document, operation),
        "set-edge" => set_edge(document, operation),
        "set-node" => set_node(document, operation),
        "set-app-name" => set_app_name(document, operation),
        _ => Err(invalid_operation(format!(
            "unsupported operation kind {kind:?}"
        ))),
    }
}

fn add_node(document: &mut Value, operation: &Map<String, Value>) -> Result<(), WorkflowDslError> {
    let id = bounded_string(operation, "id")?;
    let node_type = bounded_string(operation, "type")?;
    let parent_id = optional_non_null_string(operation, "parentId")?;
    let configuration = optional_object(operation, "configuration")?.unwrap_or_default();
    let graph = graph_mut(document)?;
    let nodes = nodes_mut(graph)?;
    if nodes
        .iter()
        .any(|node| node.get("id").and_then(Value::as_str) == Some(id))
    {
        return Err(invalid_operation(format!(
            "workflow node already exists: {id}"
        )));
    }
    if let Some(parent_id) = parent_id {
        validate_parent_placement(nodes, id, node_type, Some(parent_id))?;
    } else if is_internal_node_type(node_type) {
        return Err(invalid_operation(format!(
            "internal node type {node_type} must remain inside its matching container"
        )));
    }
    let mut data = configuration;
    data.insert("type".to_owned(), Value::String(node_type.to_owned()));
    let mut node = Map::new();
    node.insert("id".to_owned(), Value::String(id.to_owned()));
    node.insert("data".to_owned(), Value::Object(data));
    if let Some(parent_id) = parent_id {
        node.insert("parentId".to_owned(), Value::String(parent_id.to_owned()));
    }
    nodes.push(Value::Object(node));
    Ok(())
}

fn move_node(document: &mut Value, operation: &Map<String, Value>) -> Result<(), WorkflowDslError> {
    let id = bounded_string(operation, "id")?.to_owned();
    let parent_id = optional_nullable_string(operation, "parentId")?;
    let graph = graph_mut(document)?;
    let nodes = nodes_mut(graph)?;
    let index = node_index(nodes, &id)?;
    let node_type = node_type(
        nodes[index]
            .as_object()
            .ok_or_else(|| invalid_graph("workflow node is not an object"))?,
    )?
    .to_owned();
    // `null` is the explicit wire-level request to move the node to the
    // top-level scope. `optional_nullable_string` represents that state as an
    // empty string so it can also be used by edge-handle clearing; normalize it
    // before matching the node-placement state.
    match parent_id
        .as_deref()
        .filter(|parent_id| !parent_id.is_empty())
    {
        Some(parent_id) => {
            validate_parent_placement(nodes, &id, &node_type, Some(parent_id))?;
            assert_parent_cycle_free(nodes, &id, parent_id)?;
            let node = nodes[index]
                .as_object_mut()
                .ok_or_else(|| invalid_graph("workflow node is not an object"))?;
            node.insert("parentId".to_owned(), Value::String(parent_id.to_owned()));
        }
        None => {
            if is_internal_node_type(&node_type) {
                return Err(invalid_operation(format!(
                    "internal node type {node_type} must remain inside its matching container"
                )));
            }
            let node = nodes[index]
                .as_object_mut()
                .ok_or_else(|| invalid_graph("workflow node is not an object"))?;
            node.remove("parentId");
        }
    }
    Ok(())
}

fn remove_node(
    document: &mut Value,
    operation: &Map<String, Value>,
) -> Result<(), WorkflowDslError> {
    let id = bounded_string(operation, "id")?.to_owned();
    let graph = graph_mut(document)?;
    let removed = {
        let nodes = nodes_mut(graph)?;
        node_index(nodes, &id)?;
        let mut removed = BTreeSet::from([id]);
        loop {
            let mut changed = false;
            for node in nodes.iter() {
                let Some(node) = node.as_object() else {
                    return Err(invalid_graph("workflow node is not an object"));
                };
                if node
                    .get("parentId")
                    .and_then(Value::as_str)
                    .is_some_and(|parent| removed.contains(parent))
                    && removed.insert(required_object_string(node, "id")?.to_owned())
                {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        nodes.retain(|node| {
            node.get("id")
                .and_then(Value::as_str)
                .is_none_or(|node_id| !removed.contains(node_id))
        });
        removed
    };
    let edges = edges_mut(graph)?;
    edges.retain(|edge| {
        let source = edge.get("source").and_then(Value::as_str);
        let target = edge.get("target").and_then(Value::as_str);
        !source.is_some_and(|source| removed.contains(source))
            && !target.is_some_and(|target| removed.contains(target))
    });
    Ok(())
}

fn add_edge(document: &mut Value, operation: &Map<String, Value>) -> Result<(), WorkflowDslError> {
    let id = bounded_string(operation, "id")?;
    let source = bounded_string(operation, "source")?;
    let target = bounded_string(operation, "target")?;
    let source_handle = optional_non_null_string(operation, "sourceHandle")?;
    let target_handle = optional_non_null_string(operation, "targetHandle")?;
    let graph = graph_mut(document)?;
    {
        let nodes = nodes(graph)?;
        require_node(nodes, source)?;
        require_node(nodes, target)?;
    }
    let edges = edges_mut(graph)?;
    if edges
        .iter()
        .any(|edge| edge.get("id").and_then(Value::as_str) == Some(id))
    {
        return Err(invalid_operation(format!(
            "workflow edge already exists: {id}"
        )));
    }
    let mut edge = Map::new();
    edge.insert("id".to_owned(), Value::String(id.to_owned()));
    edge.insert("source".to_owned(), Value::String(source.to_owned()));
    edge.insert("target".to_owned(), Value::String(target.to_owned()));
    if let Some(value) = source_handle {
        edge.insert("sourceHandle".to_owned(), Value::String(value.to_owned()));
    }
    if let Some(value) = target_handle {
        edge.insert("targetHandle".to_owned(), Value::String(value.to_owned()));
    }
    edges.push(Value::Object(edge));
    Ok(())
}

fn remove_edge(
    document: &mut Value,
    operation: &Map<String, Value>,
) -> Result<(), WorkflowDslError> {
    let id = bounded_string(operation, "id")?;
    let graph = graph_mut(document)?;
    let edges = edges_mut(graph)?;
    let Some(index) = edges
        .iter()
        .position(|edge| edge.get("id").and_then(Value::as_str) == Some(id))
    else {
        return Err(invalid_operation(format!("workflow edge not found: {id}")));
    };
    edges.remove(index);
    Ok(())
}

fn set_edge(document: &mut Value, operation: &Map<String, Value>) -> Result<(), WorkflowDslError> {
    let id = bounded_string(operation, "id")?.to_owned();
    let source = bounded_string(operation, "source")?.to_owned();
    let target = bounded_string(operation, "target")?.to_owned();
    let source_handle = optional_nullable_string(operation, "sourceHandle")?;
    let target_handle = optional_nullable_string(operation, "targetHandle")?;
    let graph = graph_mut(document)?;
    {
        let nodes = nodes(graph)?;
        require_node(nodes, &source)?;
        require_node(nodes, &target)?;
    }
    let edges = edges_mut(graph)?;
    let Some(edge) = edges
        .iter_mut()
        .find(|edge| edge.get("id").and_then(Value::as_str) == Some(id.as_str()))
    else {
        return Err(invalid_operation(format!("workflow edge not found: {id}")));
    };
    let edge = edge
        .as_object_mut()
        .ok_or_else(|| invalid_graph("workflow edge is not an object"))?;
    edge.insert("source".to_owned(), Value::String(source));
    edge.insert("target".to_owned(), Value::String(target));
    update_nullable_string(edge, "sourceHandle", source_handle)?;
    update_nullable_string(edge, "targetHandle", target_handle)?;
    Ok(())
}

fn set_node(document: &mut Value, operation: &Map<String, Value>) -> Result<(), WorkflowDslError> {
    let id = bounded_string(operation, "id")?.to_owned();
    let configuration = required_object(operation, "configuration")?;
    let graph = graph_mut(document)?;
    let nodes = nodes_mut(graph)?;
    let index = node_index(nodes, &id)?;
    let node = nodes[index]
        .as_object_mut()
        .ok_or_else(|| invalid_graph("workflow node is not an object"))?;
    let data = node
        .get_mut("data")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid_graph("workflow node.data is not an object"))?;
    let node_type = data
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_graph("workflow node.data.type is missing"))?
        .to_owned();
    for (key, value) in configuration {
        if key != "type" {
            data.insert(key.clone(), value.clone());
        }
    }
    data.insert("type".to_owned(), Value::String(node_type));
    Ok(())
}

fn set_app_name(
    document: &mut Value,
    operation: &Map<String, Value>,
) -> Result<(), WorkflowDslError> {
    // The operation byte budget bounds display text; the 255-byte identity
    // limit is intentionally reserved for node, edge, endpoint, and type
    // strings so long human-readable application names remain representable.
    let name = required_string(operation, "name")?;
    let app = document
        .get_mut("app")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid_graph("workflow document.app is not an object"))?;
    app.insert("name".to_owned(), Value::String(name.to_owned()));
    Ok(())
}

fn graph_mut(document: &mut Value) -> Result<&mut Map<String, Value>, WorkflowDslError> {
    document
        .get_mut("workflow")
        .and_then(Value::as_object_mut)
        .and_then(|workflow| workflow.get_mut("graph"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invalid_graph("workflow.graph is not an object"))
}

fn nodes(graph: &Map<String, Value>) -> Result<&Vec<Value>, WorkflowDslError> {
    graph
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_graph("workflow.graph.nodes is not an array"))
}

fn nodes_mut(graph: &mut Map<String, Value>) -> Result<&mut Vec<Value>, WorkflowDslError> {
    graph
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_graph("workflow.graph.nodes is not an array"))
}

fn edges_mut(graph: &mut Map<String, Value>) -> Result<&mut Vec<Value>, WorkflowDslError> {
    graph
        .get_mut("edges")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid_graph("workflow.graph.edges is not an array"))
}

fn node_index(nodes: &[Value], id: &str) -> Result<usize, WorkflowDslError> {
    nodes
        .iter()
        .position(|node| node.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| invalid_operation(format!("workflow node not found: {id}")))
}

fn require_node<'a>(nodes: &'a [Value], id: &str) -> Result<&'a Value, WorkflowDslError> {
    let index = node_index(nodes, id)?;
    Ok(&nodes[index])
}

fn node_type(node: &Map<String, Value>) -> Result<&str, WorkflowDslError> {
    node.get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("type"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_graph("workflow node.data.type is missing"))
}

fn validate_parent_placement(
    nodes: &[Value],
    node_id: &str,
    candidate_type: &str,
    parent_id: Option<&str>,
) -> Result<(), WorkflowDslError> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    if parent_id == node_id {
        return Err(invalid_operation(format!(
            "workflow node {node_id} cannot be its own parent"
        )));
    }
    let parent = require_node(nodes, parent_id)?;
    let parent_type = node_type(
        parent
            .as_object()
            .ok_or_else(|| invalid_graph("workflow node is not an object"))?,
    )?;
    if !matches!(parent_type, "iteration" | "loop") {
        return Err(invalid_operation(format!(
            "workflow node parent must be an iteration or loop container: {parent_id}"
        )));
    }
    if (candidate_type == "iteration-start" && parent_type != "iteration")
        || (candidate_type == "loop-start" && parent_type != "loop")
    {
        return Err(invalid_operation(format!(
            "internal node {candidate_type} must belong to the matching container: {parent_id}"
        )));
    }
    Ok(())
}

fn assert_parent_cycle_free(
    nodes: &[Value],
    node_id: &str,
    parent_id: &str,
) -> Result<(), WorkflowDslError> {
    let mut current = parent_id;
    let mut visited = BTreeSet::new();
    loop {
        if current == node_id || !visited.insert(current.to_owned()) {
            return Err(invalid_operation(format!(
                "moving workflow node {node_id} would create a parent cycle"
            )));
        }
        let node = require_node(nodes, current)?;
        let Some(parent) = node.get("parentId").and_then(Value::as_str) else {
            return Ok(());
        };
        current = parent;
    }
}

fn is_internal_node_type(node_type: &str) -> bool {
    matches!(node_type, "iteration-start" | "loop-start")
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, WorkflowDslError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_operation(format!("operation.{key} must be a non-empty string")))
}

fn required_object<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Map<String, Value>, WorkflowDslError> {
    object
        .get(key)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_operation(format!("operation.{key} must be a JSON object")))
}

fn optional_object(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<Map<String, Value>>, WorkflowDslError> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    value
        .as_object()
        .cloned()
        .map(Some)
        .ok_or_else(|| invalid_operation(format!("operation.{key} must be a JSON object")))
}

fn bounded_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, WorkflowDslError> {
    let value = required_string(object, key)?;
    if value.len() > WORKFLOW_AUTHORING_ID_MAX_BYTES {
        return Err(invalid_operation(format!(
            "operation.{key} exceeds {WORKFLOW_AUTHORING_ID_MAX_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn required_object_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, WorkflowDslError> {
    bounded_string(object, key)
}

fn optional_non_null_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, WorkflowDslError> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Err(invalid_operation(format!(
            "operation.{key} must be a string"
        )));
    }
    bounded_string(object, key).map(Some)
}

/// `None` means the operation omitted the property; `Some("")` means it
/// explicitly supplied JSON null.
fn optional_nullable_string(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, WorkflowDslError> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(String::new()));
    }
    Ok(Some(bounded_string(object, key)?.to_owned()))
}

fn update_nullable_string(
    object: &mut Map<String, Value>,
    key: &str,
    value: Option<String>,
) -> Result<(), WorkflowDslError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_empty() {
        object.remove(key);
    } else {
        object.insert(key.to_owned(), Value::String(value));
    }
    Ok(())
}

fn invalid_operation(message: impl Into<String>) -> WorkflowDslError {
    WorkflowDslError::InvalidAuthoringOperation {
        message: message.into(),
    }
}

fn invalid_graph(message: impl Into<String>) -> WorkflowDslError {
    WorkflowDslError::InvalidGraph {
        message: message.into(),
    }
}

fn serialization_error(error: serde_json::Error) -> WorkflowDslError {
    WorkflowDslError::Serialization {
        message: error.to_string(),
    }
}
