use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{WorkflowDagPlan, WorkflowDslCompatibility, WorkflowDslError};

pub const WORKFLOW_DSL_MAX_BYTES: usize = 10 * 1024 * 1024;
pub const TESTED_WORKFLOW_DSL_VERSION: &str = "0.7.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDsl {
    version: String,
    kind: String,
    app: WorkflowDslApp,
    #[serde(default)]
    dependencies: Vec<Value>,
    workflow: WorkflowDslBody,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl WorkflowDsl {
    pub fn from_yaml(source: &str) -> Result<Self, WorkflowDslError> {
        check_size(source)?;
        let document: Self =
            serde_yaml_ng::from_str(source).map_err(|error| WorkflowDslError::InvalidYaml {
                message: error.to_string(),
            })?;
        document.validate_document()?;
        Ok(document)
    }

    pub fn from_json(source: &str) -> Result<Self, WorkflowDslError> {
        check_size(source)?;
        let document: Self =
            serde_json::from_str(source).map_err(|error| WorkflowDslError::InvalidJson {
                message: error.to_string(),
            })?;
        document.validate_document()?;
        Ok(document)
    }

    pub fn to_yaml(&self) -> Result<String, WorkflowDslError> {
        serde_yaml_ng::to_string(self).map_err(|error| WorkflowDslError::Serialization {
            message: error.to_string(),
        })
    }

    pub fn to_json(&self) -> Result<String, WorkflowDslError> {
        serde_json::to_string(self).map_err(|error| WorkflowDslError::Serialization {
            message: error.to_string(),
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn app(&self) -> &WorkflowDslApp {
        &self.app
    }

    pub fn dependencies(&self) -> &[Value] {
        &self.dependencies
    }

    pub fn workflow(&self) -> &WorkflowDslBody {
        &self.workflow
    }

    pub fn graph(&self) -> &WorkflowDag {
        &self.workflow.graph
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    pub fn execution_digest(&self) -> Result<String, WorkflowDslError> {
        super::digest::document_execution_digest(self)
    }

    /// Classify the imported DSL version. Newer releases and a different major
    /// need explicit confirmation, while an older minor remains importable
    /// with warnings.
    pub fn compatibility(&self) -> Result<WorkflowDslCompatibility, WorkflowDslError> {
        super::version::classify_dsl_version(&self.version)
    }

    fn validate_document(&self) -> Result<(), WorkflowDslError> {
        if self.version.trim().is_empty() {
            return Err(invalid_document("version is empty"));
        }
        if self.kind != "app" {
            return Err(invalid_document(format!(
                "kind {:?} is not a workflow app",
                self.kind
            )));
        }
        if self.app.name.trim().is_empty() {
            return Err(invalid_document("app.name is empty"));
        }
        if !matches!(self.app.mode.as_str(), "workflow" | "advanced-chat") {
            return Err(invalid_document(format!(
                "app.mode {:?} does not contain a Workflow graph",
                self.app.mode
            )));
        }
        self.compatibility()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDslApp {
    name: String,
    mode: String,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl WorkflowDslApp {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mode(&self) -> &str {
        &self.mode
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDslBody {
    graph: WorkflowDag,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl WorkflowDslBody {
    pub fn graph(&self) -> &WorkflowDag {
        &self.graph
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDag {
    #[serde(default)]
    nodes: Vec<WorkflowDagNode>,
    #[serde(default)]
    edges: Vec<WorkflowDagEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    viewport: Option<Value>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl WorkflowDag {
    /// Construct a DAG programmatically. Hosts that compile an authoritative
    /// product format can use this path without serializing through YAML or
    /// JSON first.
    pub fn new(nodes: Vec<WorkflowDagNode>, edges: Vec<WorkflowDagEdge>) -> Self {
        Self {
            nodes,
            edges,
            viewport: None,
            extensions: BTreeMap::new(),
        }
    }

    pub fn from_json(source: &str) -> Result<Self, WorkflowDslError> {
        check_size(source)?;
        serde_json::from_str(source).map_err(|error| WorkflowDslError::InvalidJson {
            message: error.to_string(),
        })
    }

    pub fn from_yaml(source: &str) -> Result<Self, WorkflowDslError> {
        check_size(source)?;
        serde_yaml_ng::from_str(source).map_err(|error| WorkflowDslError::InvalidYaml {
            message: error.to_string(),
        })
    }

    pub fn to_json(&self) -> Result<String, WorkflowDslError> {
        serde_json::to_string(self).map_err(|error| WorkflowDslError::Serialization {
            message: error.to_string(),
        })
    }

    pub fn nodes(&self) -> &[WorkflowDagNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[WorkflowDagEdge] {
        &self.edges
    }

    pub fn viewport(&self) -> Option<&Value> {
        self.viewport.as_ref()
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    pub fn node(&self, id: &str) -> Option<&WorkflowDagNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn execution_plan(&self) -> Result<WorkflowDagPlan, WorkflowDslError> {
        super::plan::build_execution_plan(self)
    }

    /// Derive a stable identity for executable graph semantics. Canvas layout
    /// and authoring order do not affect this digest.
    pub fn execution_digest(&self) -> Result<String, WorkflowDslError> {
        super::digest::graph_execution_digest(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDagNode {
    id: String,
    data: Value,
    #[serde(rename = "parentId", default, skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    #[serde(flatten)]
    presentation: BTreeMap<String, Value>,
}

impl WorkflowDagNode {
    /// Construct a node with the canonical `data.type` discriminator.
    pub fn new(id: impl Into<String>, node_type: impl Into<String>) -> Self {
        Self::from_data(
            id,
            Value::Object(serde_json::Map::from_iter([(
                "type".to_owned(),
                Value::String(node_type.into()),
            )])),
        )
    }

    /// Construct a node from its complete semantic data object.
    pub fn from_data(id: impl Into<String>, data: Value) -> Self {
        Self {
            id: id.into(),
            data,
            parent_id: None,
            presentation: BTreeMap::new(),
        }
    }

    /// Place this node inside an iteration or loop container scope.
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn node_type(&self) -> &str {
        self.data.get("type").and_then(Value::as_str).unwrap_or("")
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }

    pub fn presentation(&self) -> &BTreeMap<String, Value> {
        &self.presentation
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDagEdge {
    id: String,
    source: String,
    target: String,
    #[serde(
        rename = "sourceHandle",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    source_handle: Option<String>,
    #[serde(
        rename = "targetHandle",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    target_handle: Option<String>,
    #[serde(default)]
    data: Value,
    #[serde(flatten)]
    presentation: BTreeMap<String, Value>,
}

impl WorkflowDagEdge {
    /// Construct a directed edge between two node identities.
    pub fn new(
        id: impl Into<String>,
        source: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            target: target.into(),
            source_handle: None,
            target_handle: None,
            data: Value::Null,
            presentation: BTreeMap::new(),
        }
    }

    pub fn with_source_handle(mut self, source_handle: impl Into<String>) -> Self {
        self.source_handle = Some(source_handle.into());
        self
    }

    pub fn with_target_handle(mut self, target_handle: impl Into<String>) -> Self {
        self.target_handle = Some(target_handle.into());
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn source_handle(&self) -> Option<&str> {
        self.source_handle.as_deref()
    }

    pub fn target_handle(&self) -> Option<&str> {
        self.target_handle.as_deref()
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn presentation(&self) -> &BTreeMap<String, Value> {
        &self.presentation
    }
}

fn check_size(source: &str) -> Result<(), WorkflowDslError> {
    let actual_bytes = source.len();
    if actual_bytes > WORKFLOW_DSL_MAX_BYTES {
        return Err(WorkflowDslError::DocumentTooLarge {
            actual_bytes,
            maximum_bytes: WORKFLOW_DSL_MAX_BYTES,
        });
    }
    Ok(())
}

fn invalid_document(message: impl Into<String>) -> WorkflowDslError {
    WorkflowDslError::InvalidDocument {
        message: message.into(),
    }
}
