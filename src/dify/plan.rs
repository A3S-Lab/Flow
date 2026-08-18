use std::collections::{BTreeMap, BTreeSet};

use super::{DifyGraph, DifyImportError};

const MAX_DIFY_NODES: usize = 10_000;
const MAX_DIFY_EDGES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifyExecutionPlan {
    scopes: BTreeMap<Option<String>, Vec<String>>,
}

impl DifyExecutionPlan {
    pub fn top_level(&self) -> &[String] {
        self.scopes.get(&None).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn scope(&self, parent_id: &str) -> Option<&[String]> {
        self.scopes
            .get(&Some(parent_id.to_owned()))
            .map(Vec::as_slice)
    }

    pub fn scopes(&self) -> &BTreeMap<Option<String>, Vec<String>> {
        &self.scopes
    }
}

pub(super) fn build_execution_plan(
    graph: &DifyGraph,
) -> Result<DifyExecutionPlan, DifyImportError> {
    if graph.nodes().is_empty() {
        return Err(invalid_graph(
            "an executable graph requires at least one node",
        ));
    }
    if graph.nodes().len() > MAX_DIFY_NODES {
        return Err(invalid_graph(format!(
            "node count {} exceeds {MAX_DIFY_NODES}",
            graph.nodes().len()
        )));
    }
    if graph.edges().len() > MAX_DIFY_EDGES {
        return Err(invalid_graph(format!(
            "edge count {} exceeds {MAX_DIFY_EDGES}",
            graph.edges().len()
        )));
    }

    let mut nodes = BTreeMap::new();
    let mut scope_nodes: BTreeMap<Option<String>, BTreeSet<String>> = BTreeMap::new();
    for node in graph.nodes() {
        if node.id().trim().is_empty() {
            return Err(invalid_graph("node ID is empty"));
        }
        if node.node_type().trim().is_empty() {
            return Err(invalid_graph(format!(
                "node {:?} has no string data.type",
                node.id()
            )));
        }
        if nodes.insert(node.id(), node).is_some() {
            return Err(invalid_graph(format!("duplicate node ID {:?}", node.id())));
        }
        scope_nodes
            .entry(node.parent_id().map(str::to_owned))
            .or_default()
            .insert(node.id().to_owned());
    }
    validate_container_scopes(&nodes)?;

    let mut edge_ids = BTreeSet::new();
    let mut outgoing: BTreeMap<&str, Vec<&str>> =
        nodes.keys().copied().map(|id| (id, Vec::new())).collect();
    let mut indegree: BTreeMap<&str, usize> = nodes.keys().copied().map(|id| (id, 0)).collect();
    for edge in graph.edges() {
        if edge.id().trim().is_empty() {
            return Err(invalid_graph("edge ID is empty"));
        }
        if !edge_ids.insert(edge.id()) {
            return Err(invalid_graph(format!("duplicate edge ID {:?}", edge.id())));
        }
        let source = nodes.get(edge.source()).ok_or_else(|| {
            invalid_graph(format!(
                "edge {:?} references missing source {:?}",
                edge.id(),
                edge.source()
            ))
        })?;
        let target = nodes.get(edge.target()).ok_or_else(|| {
            invalid_graph(format!(
                "edge {:?} references missing target {:?}",
                edge.id(),
                edge.target()
            ))
        })?;
        if edge.source() == edge.target() {
            return Err(invalid_graph(format!(
                "edge {:?} connects a node to itself",
                edge.id()
            )));
        }
        if source.parent_id() != target.parent_id() {
            return Err(invalid_graph(format!(
                "edge {:?} crosses Dify graph scopes",
                edge.id()
            )));
        }
        outgoing
            .get_mut(edge.source())
            .ok_or_else(|| invalid_graph("validated edge source has no adjacency state"))?
            .push(edge.target());
        let target_indegree = indegree
            .get_mut(edge.target())
            .ok_or_else(|| invalid_graph("validated edge target has no indegree state"))?;
        *target_indegree = target_indegree
            .checked_add(1)
            .ok_or_else(|| invalid_graph("Dify graph indegree overflowed"))?;
    }

    for targets in outgoing.values_mut() {
        targets.sort_unstable();
    }
    let mut plans = BTreeMap::new();
    for (scope, ids) in scope_nodes {
        let mut scoped_indegree = BTreeMap::new();
        for id in &ids {
            let count = indegree
                .get(id.as_str())
                .copied()
                .ok_or_else(|| invalid_graph(format!("node {id:?} has no indegree state")))?;
            scoped_indegree.insert(id.as_str(), count);
        }
        let mut ready = scoped_indegree
            .iter()
            .filter_map(|(id, count)| (*count == 0).then_some(*id))
            .collect::<BTreeSet<_>>();
        let mut order = Vec::with_capacity(ids.len());
        while let Some(id) = ready.pop_first() {
            order.push(id.to_owned());
            let targets = outgoing
                .get(id)
                .ok_or_else(|| invalid_graph(format!("node {id:?} has no adjacency state")))?;
            for target in targets {
                let count = scoped_indegree.get_mut(target).ok_or_else(|| {
                    invalid_graph(format!(
                        "same-scope target {target:?} has no indegree state"
                    ))
                })?;
                *count = count
                    .checked_sub(1)
                    .ok_or_else(|| invalid_graph("Dify graph indegree underflowed"))?;
                if *count == 0 {
                    ready.insert(target);
                }
            }
        }
        if order.len() != ids.len() {
            return Err(invalid_graph(match scope.as_deref() {
                Some(parent_id) => format!("scope {parent_id:?} contains a cycle"),
                None => "top-level graph contains a cycle".to_owned(),
            }));
        }
        plans.insert(scope, order);
    }

    Ok(DifyExecutionPlan { scopes: plans })
}

fn validate_container_scopes(
    nodes: &BTreeMap<&str, &super::DifyNode>,
) -> Result<(), DifyImportError> {
    for node in nodes.values() {
        let Some(parent_id) = node.parent_id() else {
            continue;
        };
        let parent = nodes.get(parent_id).ok_or_else(|| {
            invalid_graph(format!(
                "node {:?} references missing parent {parent_id:?}",
                node.id()
            ))
        })?;
        let parent_type = parent.node_type();
        if !matches!(parent_type, "iteration" | "loop") {
            return Err(invalid_graph(format!(
                "node {:?} parent {parent_id:?} is not an iteration or loop",
                node.id()
            )));
        }
        match (parent_type, node.node_type()) {
            ("iteration", "loop-start") => {
                return Err(invalid_graph(format!(
                    "iteration {parent_id:?} requires an iteration-start child, not {:?}",
                    node.id()
                )))
            }
            ("loop", "iteration-start") => {
                return Err(invalid_graph(format!(
                    "loop {parent_id:?} requires a loop-start child, not {:?}",
                    node.id()
                )))
            }
            _ => {}
        }
    }

    for node in nodes.values() {
        let expected_start_type = match node.node_type() {
            "iteration" => "iteration-start",
            "loop" => "loop-start",
            _ => continue,
        };
        let start_node_id = node
            .data()
            .get("start_node_id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                invalid_graph(format!(
                    "{} container {:?} has no string data.start_node_id",
                    node.node_type(),
                    node.id()
                ))
            })?;
        let start = nodes.get(start_node_id).ok_or_else(|| {
            invalid_graph(format!(
                "{} container {:?} references missing start node {start_node_id:?}",
                node.node_type(),
                node.id()
            ))
        })?;
        if start.parent_id() != Some(node.id()) || start.node_type() != expected_start_type {
            return Err(invalid_graph(format!(
                "{} container {:?} requires an {expected_start_type} child referenced by data.start_node_id",
                node.node_type(),
                node.id()
            )));
        }
        if !nodes.values().any(|candidate| {
            candidate.parent_id() == Some(node.id()) && candidate.id() != start_node_id
        }) {
            return Err(invalid_graph(format!(
                "{} container {:?} has no executable child",
                node.node_type(),
                node.id()
            )));
        }
    }

    for node in nodes.values() {
        let mut current = node;
        let mut ancestors = BTreeSet::new();
        while let Some(parent_id) = current.parent_id() {
            if !ancestors.insert(parent_id) {
                return Err(invalid_graph(format!(
                    "node {:?} has a cycle in its parentId chain",
                    node.id()
                )));
            }
            current = nodes.get(parent_id).ok_or_else(|| {
                invalid_graph(format!(
                    "node {:?} references missing parent {parent_id:?}",
                    current.id()
                ))
            })?;
        }
    }
    Ok(())
}

fn invalid_graph(message: impl Into<String>) -> DifyImportError {
    DifyImportError::InvalidGraph {
        message: message.into(),
    }
}
