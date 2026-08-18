use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{DifyDslCompatibility, DifyExecutionPlan, DifyImportError};

pub const DIFY_DSL_MAX_BYTES: usize = 10 * 1024 * 1024;
pub const DIFY_TESTED_DSL_VERSION: &str = "0.7.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifyAppDsl {
    version: String,
    kind: String,
    app: DifyAppMetadata,
    #[serde(default)]
    dependencies: Vec<Value>,
    workflow: DifyWorkflow,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl DifyAppDsl {
    pub fn from_yaml(source: &str) -> Result<Self, DifyImportError> {
        check_size(source)?;
        let document: Self =
            serde_yaml_ng::from_str(source).map_err(|error| DifyImportError::InvalidYaml {
                message: error.to_string(),
            })?;
        document.validate_document()?;
        Ok(document)
    }

    pub fn from_json(source: &str) -> Result<Self, DifyImportError> {
        check_size(source)?;
        let document: Self =
            serde_json::from_str(source).map_err(|error| DifyImportError::InvalidJson {
                message: error.to_string(),
            })?;
        document.validate_document()?;
        Ok(document)
    }

    pub fn to_yaml(&self) -> Result<String, DifyImportError> {
        serde_yaml_ng::to_string(self).map_err(|error| DifyImportError::Serialization {
            message: error.to_string(),
        })
    }

    pub fn to_json(&self) -> Result<String, DifyImportError> {
        serde_json::to_string(self).map_err(|error| DifyImportError::Serialization {
            message: error.to_string(),
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn app(&self) -> &DifyAppMetadata {
        &self.app
    }

    pub fn dependencies(&self) -> &[Value] {
        &self.dependencies
    }

    pub fn workflow(&self) -> &DifyWorkflow {
        &self.workflow
    }

    pub fn graph(&self) -> &DifyGraph {
        &self.workflow.graph
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    pub fn execution_digest(&self) -> Result<String, DifyImportError> {
        super::digest::document_execution_digest(self)
    }

    /// Classify the imported DSL version with the same compatibility rules as
    /// Dify: newer releases and a different major need explicit confirmation,
    /// while an older minor remains importable with warnings.
    pub fn compatibility(&self) -> Result<DifyDslCompatibility, DifyImportError> {
        super::version::classify_dsl_version(&self.version)
    }

    fn validate_document(&self) -> Result<(), DifyImportError> {
        if self.version.trim().is_empty() {
            return Err(invalid_document("version is empty"));
        }
        if self.kind != "app" {
            return Err(invalid_document(format!(
                "kind {:?} is not a Dify app",
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
pub struct DifyAppMetadata {
    name: String,
    mode: String,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl DifyAppMetadata {
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
pub struct DifyWorkflow {
    graph: DifyGraph,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl DifyWorkflow {
    pub fn graph(&self) -> &DifyGraph {
        &self.graph
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifyGraph {
    #[serde(default)]
    nodes: Vec<DifyNode>,
    #[serde(default)]
    edges: Vec<DifyEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    viewport: Option<Value>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl DifyGraph {
    pub fn from_json(source: &str) -> Result<Self, DifyImportError> {
        check_size(source)?;
        serde_json::from_str(source).map_err(|error| DifyImportError::InvalidJson {
            message: error.to_string(),
        })
    }

    pub fn from_yaml(source: &str) -> Result<Self, DifyImportError> {
        check_size(source)?;
        serde_yaml_ng::from_str(source).map_err(|error| DifyImportError::InvalidYaml {
            message: error.to_string(),
        })
    }

    pub fn to_json(&self) -> Result<String, DifyImportError> {
        serde_json::to_string(self).map_err(|error| DifyImportError::Serialization {
            message: error.to_string(),
        })
    }

    pub fn nodes(&self) -> &[DifyNode] {
        &self.nodes
    }

    pub fn edges(&self) -> &[DifyEdge] {
        &self.edges
    }

    pub fn viewport(&self) -> Option<&Value> {
        self.viewport.as_ref()
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    pub fn node(&self, id: &str) -> Option<&DifyNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn execution_plan(&self) -> Result<DifyExecutionPlan, DifyImportError> {
        super::plan::build_execution_plan(self)
    }

    /// Derive a stable identity for executable graph semantics. React Flow
    /// canvas layout and authoring order do not affect this digest.
    pub fn execution_digest(&self) -> Result<String, DifyImportError> {
        super::digest::graph_execution_digest(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifyNode {
    id: String,
    data: Value,
    #[serde(rename = "parentId", default, skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    #[serde(flatten)]
    presentation: BTreeMap<String, Value>,
}

impl DifyNode {
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
pub struct DifyEdge {
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

impl DifyEdge {
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

fn check_size(source: &str) -> Result<(), DifyImportError> {
    let actual_bytes = source.len();
    if actual_bytes > DIFY_DSL_MAX_BYTES {
        return Err(DifyImportError::DocumentTooLarge {
            actual_bytes,
            maximum_bytes: DIFY_DSL_MAX_BYTES,
        });
    }
    Ok(())
}

fn invalid_document(message: impl Into<String>) -> DifyImportError {
    DifyImportError::InvalidDocument {
        message: message.into(),
    }
}
