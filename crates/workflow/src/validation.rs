//! OWS document validation (validation layer).
//!
//! Structural + configuration validation of an `OwsDocument` against the
//! catalog: unknown functions, dangling `then` references, unknown trigger
//! events, missing tasks, and flow cycles. Runs both when a draft is saved and
//! before an execution starts.

use crate::model::{
    function, is_trigger_event, task_entries, OwsDocument, OwsValidation, OwsValidationIssue,
};
use serverless_workflow_core::models::task::TaskDefinition;

/// Validate an OWS document. Returns `valid == true` only when no issues exist.
pub fn validate(doc: &OwsDocument, _registry: &crate::node::NodeRegistry) -> OwsValidation {
    let mut errors: Vec<OwsValidationIssue> = Vec::new();
    let names = doc.task_names();

    if names.is_empty() {
        errors.push(OwsValidationIssue {
            task_name: None,
            code: "empty_workflow".into(),
            message: "Workflow has no tasks.".into(),
        });
    }

    let has_schedule = doc.definition.schedule.is_some();
    let has_manual = !has_schedule;
    if !has_schedule && names.is_empty() {
        // fine — manual start of an empty workflow is reported above.
    }

    // Validate every task.
    for (name, task) in task_entries(&doc.definition) {
        match task {
            TaskDefinition::Call(t) => {
                if !function::is_known(&t.call) {
                    errors.push(OwsValidationIssue {
                        task_name: Some(name.clone()),
                        code: "unknown_function".into(),
                        message: format!("Task '{name}' calls unknown function '{}'.", t.call),
                    });
                }
            }
            TaskDefinition::Switch(sw) => {
                for e in &sw.switch.entries {
                    if let Some(case) = e.values().next() {
                        if let Some(then) = &case.then {
                            if !then.is_empty() && !names.contains(then) {
                                errors.push(OwsValidationIssue {
                                    task_name: Some(name.clone()),
                                    code: "unknown_task_reference".into(),
                                    message: format!(
                                        "Switch task '{name}' case targets unknown task '{then}'."
                                    ),
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Validate `then` transitions.
    for name in crate::graph::referenced_tasks(doc) {
        if !names.contains(&name) {
            errors.push(OwsValidationIssue {
                task_name: None,
                code: "unknown_task_reference".into(),
                message: format!("Task flow references unknown task '{name}'."),
            });
        }
    }

    // Validate schedule events are known trigger events.
    if let Some(schedule) = &doc.definition.schedule {
        if let Some(on) = &schedule.on {
            let mut event_types = Vec::new();
            for f in on.all.iter().flat_map(|v| v.iter()) {
                event_types.push(f);
            }
            for f in on.any.iter().flat_map(|v| v.iter()) {
                event_types.push(f);
            }
            if let Some(one) = &on.one {
                event_types.push(one);
            }
            for filter in event_types {
                if let Some(with) = &filter.with {
                    if let Some(ty) = with.get("type").and_then(|v| v.as_str()) {
                        if !is_trigger_event(ty) {
                            errors.push(OwsValidationIssue {
                                task_name: None,
                                code: "unknown_event".into(),
                                message: format!("Unknown trigger event type '{ty}'."),
                            });
                        }
                    }
                }
            }
        }
    }

    // Flow cycles.
    if let Err(msg) = crate::graph::execution_sequence(doc) {
        errors.push(OwsValidationIssue {
            task_name: None,
            code: "cycle".into(),
            message: msg,
        });
    }

    OwsValidation {
        valid: errors.is_empty(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OwsDocument;
    use serverless_workflow_core::models::task::{CallTaskDefinition, TaskDefinition};
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
    fn valid_document_passes() {
        let doc = doc_with(vec![
            ("a".to_string(), call("cms.getContent")),
            ("b".to_string(), call("core.transform")),
        ]);
        let reg = crate::node::NodeRegistry::builtin();
        let v = validate(&doc, &reg);
        assert!(v.valid, "{v:?}");
    }

    #[test]
    fn unknown_function_detected() {
        let doc = doc_with(vec![("a".to_string(), call("doesNotExist"))]);
        let reg = crate::node::NodeRegistry::builtin();
        let v = validate(&doc, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "unknown_function"));
    }

    #[test]
    fn unknown_then_reference_detected() {
        let mut a = call("core.noop");
        if let TaskDefinition::Call(c) = &mut a {
            c.common.then = Some("ghost".to_string());
        }
        let doc = doc_with(vec![("a".to_string(), a)]);
        let reg = crate::node::NodeRegistry::builtin();
        let v = validate(&doc, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "unknown_task_reference"));
    }

    #[test]
    fn empty_workflow_detected() {
        let doc = doc_with(vec![]);
        let reg = crate::node::NodeRegistry::builtin();
        let v = validate(&doc, &reg);
        assert!(!v.valid);
        assert!(v.errors.iter().any(|e| e.code == "empty_workflow"));
    }
}
