//! Workflow validation (validation layer).
//!
//! Structural + configuration validation of a workflow against the node
//! registry: unknown node types, missing required parameters, dangling
//! connections, missing trigger nodes, and the graph being acyclic/connected
//! enough to execute. This runs both when a draft is saved and before an
//! execution starts.

use crate::node::NodeRegistry;
use crate::model::{ValidationIssue, Workflow, WorkflowValidation};

/// Validate a workflow. Returns `valid == true` only when there are no issues.
pub fn validate(workflow: &Workflow, registry: &NodeRegistry) -> WorkflowValidation {
    let mut errors: Vec<ValidationIssue> = Vec::new();
    let node_ids: Vec<&str> = workflow.nodes.iter().map(|n| n.id.as_str()).collect();

    // A workflow must have at least one node and at least one trigger.
    if workflow.nodes.is_empty() {
        errors.push(ValidationIssue {
            node_id: None,
            node_name: None,
            code: "empty_workflow".into(),
            message: "Workflow has no nodes.".into(),
        });
        return WorkflowValidation {
            valid: false,
            errors,
        };
    }

    if workflow.trigger_nodes().is_empty() {
        errors.push(ValidationIssue {
            node_id: None,
            node_name: None,
            code: "missing_trigger".into(),
            message: "Workflow has no trigger node.".into(),
        });
    }

    // Duplicate node ids.
    for (i, a) in node_ids.iter().enumerate() {
        if node_ids[..i].contains(a) {
            errors.push(ValidationIssue {
                node_id: Some((*a).to_string()),
                node_name: None,
                code: "duplicate_node_id".into(),
                message: format!("Duplicate node id '{a}'."),
            });
        }
    }

    for node in &workflow.nodes {
        let def = registry.get(&node.node_type);
        match def {
            None => errors.push(ValidationIssue {
                node_id: Some(node.id.clone()),
                node_name: Some(node.name.clone()),
                code: "unknown_node_type".into(),
                message: format!("Unknown node type '{}'.", node.node_type),
            }),
            Some(def) => {
                // Required parameters.
                for field in &def.fields {
                    if field.required {
                        let present = node
                            .parameters
                            .get(&field.name)
                            .map(|v| !v.is_null())
                            .unwrap_or(false);
                        let nonempty_str = node
                            .parameters
                            .get(&field.name)
                            .and_then(|v| v.as_str())
                            .map(|s| !s.trim().is_empty())
                            .unwrap_or(false);
                        if !present || !nonempty_str {
                            errors.push(ValidationIssue {
                                node_id: Some(node.id.clone()),
                                node_name: Some(node.name.clone()),
                                code: "missing_required_param".into(),
                                message: format!(
                                    "Node '{}' is missing required parameter '{}'.",
                                    node.name, field.label
                                ),
                            });
                        }
                    }
                }
                // Connections must respect the node's declared ports.
                for conn in &workflow.connections {
                    if conn.from == node.id {
                        let out_exists = def.outputs.iter().any(|o| o.name == conn.from_output);
                        if !out_exists {
                            errors.push(ValidationIssue {
                                node_id: Some(node.id.clone()),
                                node_name: Some(node.name.clone()),
                                code: "unknown_output_port".into(),
                                message: format!(
                                    "Connection from '{}' uses unknown output port '{}'.",
                                    node.name, conn.from_output
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    // Connections referencing unknown nodes.
    for conn in &workflow.connections {
        if !node_ids.contains(&conn.from.as_str()) {
            errors.push(ValidationIssue {
                node_id: None,
                node_name: None,
                code: "unknown_connection_source".into(),
                message: format!(
                    "Connection references unknown source node '{}'.",
                    conn.from
                ),
            });
        }
        if !node_ids.contains(&conn.to.as_str()) {
            errors.push(ValidationIssue {
                node_id: None,
                node_name: None,
                code: "unknown_connection_target".into(),
                message: format!(
                    "Connection references unknown target node '{}'.",
                    conn.to
                ),
            });
        }
    }

    // A connection must not target a trigger (triggers have no `main` input).
    for conn in &workflow.connections {
        if let Some(node) = workflow.node(&conn.to) {
            if crate::model::is_trigger_type(&node.node_type) {
                errors.push(ValidationIssue {
                    node_id: Some(node.id.clone()),
                    node_name: Some(node.name.clone()),
                    code: "cannot_connect_to_trigger".into(),
                    message: format!(
                        "Connection targets trigger node '{}' which has no input.",
                        node.name
                    ),
                });
            }
        }
    }

    // Cycle detection via DFS over the directed graph.
    if let Some(cycle) = find_cycle(workflow) {
        errors.push(ValidationIssue {
            node_id: cycle.first().cloned(),
            node_name: cycle.first().and_then(|id| workflow.node(id).map(|n| n.name.clone())),
            code: "cycle".into(),
            message: format!(
                "Workflow contains a cycle: {}.",
                cycle
                    .iter()
                    .map(|id| workflow.node(id).map(|n| n.name.clone()).unwrap_or_else(|| id.clone()))
                    .collect::<Vec<_>>()
                    .join(" → ")
            ),
        });
    }

    WorkflowValidation {
        valid: errors.is_empty(),
        errors,
    }
}

/// Detect a cycle and return a representative path of node ids (best-effort).
fn find_cycle(workflow: &Workflow) -> Option<Vec<String>> {
    use std::collections::HashMap;

    // Iterative DFS with explicit stack. Each entry tracks (node, next index).
    let mut state: HashMap<String, u8> = HashMap::new(); // 0=unvisited 1=in-stack 2=done
    let mut path: Vec<String> = Vec::new();
    let mut stack: Vec<(String, usize)> = Vec::new();

    for start in &workflow.nodes {
        if state.get(start.id.as_str()).copied().unwrap_or(0) != 0 {
            continue;
        }
        state.insert(start.id.clone(), 1);
        path.push(start.id.clone());
        stack.push((start.id.clone(), 0));

        while let Some((node_id, next_idx)) = stack.last().cloned() {
            let successors: Vec<String> = workflow
                .connections
                .iter()
                .filter(|c| c.from == node_id)
                .map(|c| c.to.clone())
                .collect();
            if next_idx < successors.len() {
                stack.last_mut().unwrap().1 += 1;
                let target = successors[next_idx].clone();
                match state.get(target.as_str()).copied().unwrap_or(0) {
                    0 => {
                        state.insert(target.clone(), 1);
                        path.push(target.clone());
                        stack.push((target, 0));
                    }
                    1 => {
                        // Back edge: reconstruct the cycle.
                        let idx = path.iter().position(|p| p == &target).unwrap_or(0);
                        return Some(path[idx..].to_vec());
                    }
                    _ => {}
                }
            } else {
                let done = stack.pop().unwrap().0;
                state.insert(done, 2);
                path.pop();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Connection, Position, WorkflowNode};

    fn node(id: &str, node_type: &str, name: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.into(),
            node_type: node_type.into(),
            name: name.into(),
            description: None,
            position: Position { x: 0.0, y: 0.0 },
            parameters: indexmap::IndexMap::new(),
            disabled: false,
            notes: None,
            credentials: vec![],
            on_error: crate::model::OnError::default(),
            error_output: None,
        }
    }

    fn conn(from: &str, from_output: &str, to: &str) -> Connection {
        Connection {
            id: format!("{from}->{to}"),
            from: from.into(),
            from_output: from_output.into(),
            to: to.into(),
            to_input: "main".into(),
            label: None,
        }
    }

    fn wf(nodes: Vec<WorkflowNode>, connections: Vec<Connection>) -> Workflow {
        let now = chrono::Utc::now();
        Workflow {
            id: 1,
            name: "Test".into(),
            description: None,
            version: 1,
            active: false,
            nodes,
            connections,
            settings: Default::default(),
            variables: Default::default(),
            tags: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn valid_workflow_passes() {
        let reg = NodeRegistry::builtin();
        let mut t = node("t", "manualTrigger", "Manual");
        t.parameters.insert("cronExpression".into(), serde_json::json!(""));
        let wf = wf(vec![t, node("n", "noop", "Noop")], vec![conn("t", "main", "n")]);
        let v = validate(&wf, &reg);
        assert!(v.valid, "{v:?}");
        assert!(v.errors.is_empty());
    }

    #[test]
    fn missing_trigger_detected() {
        let reg = NodeRegistry::builtin();
        let wf = wf(vec![node("n", "noop", "Noop")], vec![]);
        let v = validate(&wf, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "missing_trigger"));
    }

    #[test]
    fn unknown_node_type_detected() {
        let reg = NodeRegistry::builtin();
        let wf = wf(
            vec![node("t", "manualTrigger", "Manual"), node("x", "doesNotExist", "X")],
            vec![conn("t", "main", "x")],
        );
        let v = validate(&wf, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "unknown_node_type"));
    }

    #[test]
    fn missing_required_param_detected() {
        let reg = NodeRegistry::builtin();
        let mut t = node("t", "manualTrigger", "Manual");
        t.parameters.insert("cronExpression".into(), serde_json::json!(""));
        // httpRequest requires url; leave it empty.
        let mut n = node("n", "httpRequest", "HTTP");
        n.parameters.insert("method".into(), serde_json::json!("GET"));
        n.parameters.insert("url".into(), serde_json::json!(""));
        let wf = wf(vec![t, n], vec![conn("t", "main", "n")]);
        let v = validate(&wf, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "missing_required_param"));
    }

    #[test]
    fn cycle_detected() {
        let reg = NodeRegistry::builtin();
        let wf = wf(
            vec![
                node("t", "manualTrigger", "Manual"),
                node("a", "noop", "A"),
                node("b", "noop", "B"),
            ],
            vec![conn("t", "main", "a"), conn("a", "main", "b"), conn("b", "main", "a")],
        );
        let v = validate(&wf, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "cycle"));
    }

    #[test]
    fn unknown_connection_target_detected() {
        let reg = NodeRegistry::builtin();
        let wf = wf(
            vec![node("t", "manualTrigger", "Manual"), node("n", "noop", "Noop")],
            vec![conn("t", "main", "ghost")],
        );
        let v = validate(&wf, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "unknown_connection_target"));
    }
}
