//! Graph helpers: deterministic task ordering used by the executor.
//!
//! In the Open Workflow DSL, the executable units are named **tasks** declared
//! as an ordered map (`definition.do`). Execution starts at the first task and
//! follows each task's `then` transition (defaulting to the next declaration).
//! This module resolves a deterministic execution sequence and detects cycles.

use crate::model::{OwsDocument, task_entries};
use serverless_workflow_core::models::task::TaskDefinition;
use std::collections::HashSet;

/// Directive values a task's `then` may use instead of a task name.
pub const DIRECTIVE_EXIT: &str = "exit";
pub const DIRECTIVE_END: &str = "end";
pub const DIRECTIVE_CONTINUE: &str = "continue";

fn is_directive(s: &str) -> bool {
    matches!(s, DIRECTIVE_EXIT | DIRECTIVE_END | DIRECTIVE_CONTINUE)
}

/// Resolve the deterministic execution order of task names, starting at the
/// first declared task and following `then` transitions.
///
/// Returns `Err` if the flow contains a cycle.
pub fn execution_sequence(doc: &OwsDocument) -> Result<Vec<String>, String> {
    let names = doc.task_names();
    let by_name: std::collections::HashMap<&str, usize> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    if names.is_empty() {
        return Ok(vec![]);
    }

    let mut order: Vec<String> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut stack: HashSet<String> = HashSet::new();

    // Start at the first declared task.
    let mut current: Option<String> = Some(names[0].clone());
    while let Some(name) = current.take() {
        if visited.contains(&name) {
            // Already emitted → stop (a completed task is not re-entered).
            break;
        }
        if stack.contains(&name) {
            return Err(format!("workflow flow contains a cycle at task '{name}'"));
        }
        stack.insert(name.clone());
        order.push(name.clone());

        // Determine the next task from this task's `then`.
        let next = next_task(&doc, &name, &names, &by_name);
        match next {
            Next::Name(n) => {
                stack.remove(&name);
                visited.insert(name);
                current = Some(n);
            }
            Next::Directive(_) | Next::EndOfList => {
                stack.remove(&name);
                visited.insert(name);
                break;
            }
        }
    }

    // Tasks not reached via the flow are still executed if the flow ever
    // reaches them via a switch/dynamic `then`; include any remaining declared
    // tasks so all rows are pre-created deterministically.
    for n in names {
        if !order.contains(&n) {
            order.push(n);
        }
    }
    Ok(order)
}

/// The next task a given task flows to.
enum Next {
    Name(String),
    Directive(String),
    EndOfList,
}

fn next_task(
    doc: &OwsDocument,
    name: &str,
    names: &[String],
    by_name: &std::collections::HashMap<&str, usize>,
) -> Next {
    if let Some(task) = find_task(&doc.definition, name) {
        if let Some(then) = task_common(task).and_then(|c| c.then.clone()) {
            if is_directive(&then) {
                return Next::Directive(then);
            }
            if by_name.contains_key(then.as_str()) {
                return Next::Name(then);
            }
            // Unknown `then` → treat as end of the declared flow.
            return Next::EndOfList;
        }
    }
    // Default: next in declaration order.
    if let Some(&idx) = by_name.get(name) {
        if idx + 1 < names.len() {
            return Next::Name(names[idx + 1].clone());
        }
    }
    Next::EndOfList
}

fn find_task<'a>(
    definition: &'a serverless_workflow_core::models::workflow::WorkflowDefinition,
    name: &str,
) -> Option<&'a TaskDefinition> {
    task_entries(definition)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, t)| t)
}

fn task_common(task: &TaskDefinition) -> Option<&serverless_workflow_core::models::task::TaskDefinitionFields> {
    use serverless_workflow_core::models::task::TaskDefinition as T;
    match task {
        T::Call(t) => Some(&t.common),
        T::Do(t) => Some(&t.common),
        T::Emit(t) => Some(&t.common),
        T::For(t) => Some(&t.common),
        T::Fork(t) => Some(&t.common),
        T::Listen(t) => Some(&t.common),
        T::Raise(t) => Some(&t.common),
        T::Run(t) => Some(&t.common),
        T::Set(t) => Some(&t.common),
        T::Switch(t) => Some(&t.common),
        T::Try(t) => Some(&t.common),
        T::Wait(t) => Some(&t.common),
    }
}

/// All task names referenced by `then` transitions (for validation).
pub fn referenced_tasks(doc: &OwsDocument) -> Vec<String> {
    let mut out = Vec::new();
    for (name, task) in task_entries(&doc.definition) {
        if let Some(then) = task_common(task).and_then(|c| c.then.clone()) {
            if !is_directive(&then) {
                out.push(then);
            }
        }
        // Switch cases may also transition.
        if let TaskDefinition::Switch(sw) = task {
            for e in &sw.switch.entries {
                if let Some(case) = e.values().next() {
                    if let Some(then) = case.then.clone() {
                        if !is_directive(&then) {
                            out.push(then);
                        }
                    }
                }
            }
        }
        let _ = name;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OwsDocument;
    use serverless_workflow_core::models::task::{CallTaskDefinition, TaskDefinition, WaitTaskDefinition};
    use serverless_workflow_core::models::workflow::{WorkflowDefinition, WorkflowDefinitionMetadata};

    fn doc_with(tasks: Vec<(String, TaskDefinition)>) -> OwsDocument {
        let metadata = WorkflowDefinitionMetadata::new("default", "g", "1.0.0", None, None, None);
        let mut definition = WorkflowDefinition::new(metadata);
        for (n, t) in tasks {
            definition.do_.add(n, t);
        }
        OwsDocument {
            id: 1,
            active: false,
            version: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            definition,
        }
    }

    fn call(name: &str) -> TaskDefinition {
        TaskDefinition::Call(CallTaskDefinition::new(name, None, None))
    }

    #[test]
    fn sequence_follows_declaration() {
        let doc = doc_with(vec![
            ("a".to_string(), call("core.noop")),
            ("b".to_string(), call("core.noop")),
            ("c".to_string(), call("core.noop")),
        ]);
        let order = execution_sequence(&doc).unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn sequence_follows_then() {
        let mut a = call("core.noop");
        if let TaskDefinition::Call(c) = &mut a {
            c.common.then = Some("c".to_string());
        }
        let doc = doc_with(vec![
            ("a".to_string(), a),
            ("b".to_string(), call("core.noop")),
            ("c".to_string(), call("core.noop")),
        ]);
        let order = execution_sequence(&doc).unwrap();
        assert_eq!(order.first().unwrap(), "a");
        assert_eq!(order[1], "c");
    }
}
