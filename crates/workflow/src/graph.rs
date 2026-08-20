//! Graph helpers: deterministic topological ordering used by the executor.
//!
//! The executor runs nodes in topological order (Kahn's algorithm over the
//! workflow's connections), so a node always runs after everything it depends
//! on. Branching is handled at runtime: a node only processes items it
//! actually receives, and nodes with no incoming data are skipped.

use crate::model::Workflow;
use std::collections::{HashMap, VecDeque};

/// Compute a deterministic topological order of node ids.
///
/// Returns `Err` if the graph contains a cycle (the workflow is not executable).
pub fn topological_order(workflow: &Workflow) -> Result<Vec<String>, String> {
    // Node id -> list of downstream node ids (via any output port).
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut indegree: HashMap<&str, usize> = HashMap::new();
    for node in &workflow.nodes {
        indegree.entry(node.id.as_str()).or_insert(0);
        adjacency.entry(node.id.as_str()).or_default();
    }
    for conn in &workflow.connections {
        if conn.from == conn.to {
            return Err("self-loop connection".into());
        }
        adjacency
            .entry(conn.from.as_str())
            .or_default()
            .push(conn.to.as_str());
        *indegree.entry(conn.to.as_str()).or_insert(0) += 1;
    }

    let mut order: Vec<String> = Vec::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&id, &deg) in &indegree {
        if deg == 0 {
            queue.push_back(id);
        }
    }
    // Deterministic: process in sorted order for stable execution.
    let mut ready: Vec<&str> = queue.into_iter().collect();
    ready.sort_unstable();
    while let Some(id) = ready.first().copied() {
        ready.remove(0);
        order.push(id.to_string());
        let mut downstream: Vec<&str> = adjacency.get(id).cloned().unwrap_or_default();
        downstream.sort_unstable();
        for next in downstream {
            let e = indegree.get_mut(next).unwrap();
            *e -= 1;
            if *e == 0 {
                ready.push(next);
                ready.sort_unstable();
            }
        }
    }

    if order.len() != workflow.nodes.len() {
        return Err("workflow graph contains a cycle".into());
    }
    Ok(order)
}

/// The nodes reachable from a given set of trigger node ids following `main`
/// (and branch) connections — used to detect unreachable/isolated subgraphs.
pub fn reachable_from(workflow: &Workflow, roots: &[&str]) -> std::collections::HashSet<String> {
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<String> = roots.iter().map(|s| s.to_string()).collect();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        for conn in workflow.connections.iter() {
            if conn.from == id {
                stack.push(conn.to.clone());
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Connection, Position, WorkflowNode};

    fn node(id: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.into(),
            node_type: "noop".into(),
            name: id.into(),
            description: None,
            position: Position { x: 0.0, y: 0.0 },
            parameters: Default::default(),
            disabled: false,
            notes: None,
            credentials: vec![],
            on_error: crate::model::OnError::default(),
            error_output: None,
        }
    }
    fn conn(from: &str, to: &str) -> Connection {
        Connection {
            id: format!("{from}->{to}"),
            from: from.into(),
            from_output: "main".into(),
            to: to.into(),
            to_input: "main".into(),
            label: None,
        }
    }
    fn wf(nodes: Vec<WorkflowNode>, conns: Vec<Connection>) -> Workflow {
        let now = chrono::Utc::now();
        Workflow {
            id: 1,
            name: "g".into(),
            description: None,
            version: 1,
            active: false,
            nodes,
            connections: conns,
            settings: Default::default(),
            variables: Default::default(),
            tags: vec![],
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn linear_order() {
        let wf = wf(
            vec![node("a"), node("b"), node("c")],
            vec![conn("a", "b"), conn("b", "c")],
        );
        let order = topological_order(&wf).unwrap();
        assert!(order.iter().position(|x| x == "a").unwrap()
            < order.iter().position(|x| x == "b").unwrap());
        assert!(order.iter().position(|x| x == "b").unwrap()
            < order.iter().position(|x| x == "c").unwrap());
    }

    #[test]
    fn diamond_order() {
        let wf = wf(
            vec![node("start"), node("l"), node("r"), node("end")],
            vec![conn("start", "l"), conn("start", "r"), conn("l", "end"), conn("r", "end")],
        );
        let order = topological_order(&wf).unwrap();
        let end = order.iter().position(|x| x == "end").unwrap();
        assert!(order.iter().position(|x| x == "l").unwrap() < end);
        assert!(order.iter().position(|x| x == "r").unwrap() < end);
    }

    #[test]
    fn cycle_is_err() {
        let wf = wf(
            vec![node("a"), node("b")],
            vec![conn("a", "b"), conn("b", "a")],
        );
        assert!(topological_order(&wf).is_err());
    }

    #[test]
    fn reachability() {
        let wf = wf(
            vec![node("t"), node("a"), node("b")],
            vec![conn("t", "a"), conn("b", "b")], // b isolated
        );
        let reach = reachable_from(&wf, &["t"]);
        assert!(reach.contains("t"));
        assert!(reach.contains("a"));
        assert!(!reach.contains("b"));
    }
}
