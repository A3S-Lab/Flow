use a3s_flow::{
    ChildWorkflowCancellationPolicy, ChildWorkflowCommand, FlowError, FlowEvent, FlowEventEnvelope,
    NativeRuntimeKind, NativeRuntimeRequest, NativeRuntimeResponse, NativeTsCompilerCapabilities,
    NativeTsDependencyManifest, RuntimeCommand, StepInvocation, WorkflowRunSummary, WorkflowSignal,
    WorkflowSpec, WorkflowTerminalOutcome, FLOW_EVENT_ENVELOPE_SCHEMA_VERSION,
    NATIVE_COMPILER_PROTOCOL, NATIVE_DEPENDENCY_MANIFEST_PROTOCOL, NATIVE_RUNTIME_PROTOCOL,
};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

#[test]
fn native_runtime_request_uses_stable_json_field_names() {
    let request = NativeRuntimeRequest::new(
        NativeRuntimeKind::Workflow,
        "main",
        "abc123",
        json!({ "run_id": "run-1" }),
    );

    let encoded = serde_json::to_value(request).unwrap();
    assert_eq!(encoded["protocol"], NATIVE_RUNTIME_PROTOCOL);
    assert_eq!(encoded["kind"], "workflow");
    assert_eq!(encoded["exportName"], "main");
    assert_eq!(encoded["sourceHash"], "abc123");
    assert_eq!(encoded["payload"]["run_id"], "run-1");
}

#[test]
fn native_runtime_response_decodes_kind_and_error_envelope() {
    let response: NativeRuntimeResponse = serde_json::from_value(json!({
        "protocol": NATIVE_RUNTIME_PROTOCOL,
        "kind": "step",
        "ok": false,
        "error": "step failed"
    }))
    .unwrap();

    assert_eq!(response.kind, NativeRuntimeKind::Step);
    assert_eq!(response.error.as_deref(), Some("step failed"));
}

#[test]
fn activity_protocol_preserves_fencing_and_checkpoint_fields() {
    let command = RuntimeCommand::schedule_activity("a1", "sendEmail", json!({ "to": "x" }));
    let encoded = serde_json::to_value(command).unwrap();
    assert_eq!(encoded["type"], "schedule_activity");
    assert_eq!(encoded["activity_id"], "a1");

    let event = FlowEvent::ActivityHeartbeat {
        activity_id: "a1".to_string(),
        attempt: 1,
        attempt_id: "attempt-1".to_string(),
        fencing_token: "fence-1".to_string(),
        checkpoint: Some(json!({ "cursor": 9 })),
    };
    let encoded = serde_json::to_value(event).unwrap();
    assert_eq!(encoded["type"], "activity_heartbeat");
    assert_eq!(encoded["fencing_token"], "fence-1");
    assert_eq!(encoded["checkpoint"]["cursor"], 9);

    assert_eq!(
        serde_json::to_value(NativeRuntimeKind::Activity).unwrap(),
        "activity"
    );
}

#[test]
fn native_compiler_protocol_has_stable_capability_and_manifest_fields() {
    let capabilities = serde_json::to_value(NativeTsCompilerCapabilities::current()).unwrap();
    assert_eq!(capabilities["protocol"], NATIVE_COMPILER_PROTOCOL);
    assert_eq!(capabilities["dependencyManifest"], true);

    let manifest = serde_json::to_value(NativeTsDependencyManifest::new(
        "bun-sha256:test",
        vec!["src/main.ts".to_string(), "src/shared.ts".to_string()],
    ))
    .unwrap();
    assert_eq!(manifest["protocol"], NATIVE_DEPENDENCY_MANIFEST_PROTOCOL);
    assert_eq!(manifest["compilerIdentity"], "bun-sha256:test");
    assert_eq!(manifest["files"][0], "src/main.ts");
    assert_eq!(manifest["files"][1], "src/shared.ts");
}

#[test]
fn native_runtime_operation_commands_accept_omitted_optional_fields() {
    let progress: RuntimeCommand = serde_json::from_value(json!({
        "type": "record_progress",
        "progress": {
            "progress_id": "download-1",
            "completed": 1
        }
    }))
    .unwrap();
    match progress {
        RuntimeCommand::RecordProgress { progress } => {
            assert_eq!(progress.total, None);
            assert_eq!(progress.message, None);
            assert_eq!(progress.details, serde_json::Value::Null);
        }
        command => panic!("unexpected command: {command:?}"),
    }

    let child: RuntimeCommand = serde_json::from_value(json!({
        "type": "link_child_operation",
        "child": {
            "reference_id": "runtime",
            "kind": "runtime.unit",
            "operation_id": "runtime-1"
        }
    }))
    .unwrap();
    match child {
        RuntimeCommand::LinkChildOperation { child } => {
            assert_eq!(child.flow_run_id, None);
            assert_eq!(child.metadata, serde_json::Value::Null);
        }
        command => panic!("unexpected command: {command:?}"),
    }

    let continuation: RuntimeCommand = serde_json::from_value(json!({
        "type": "continue_as_new",
        "input": { "cursor": "next-page" }
    }))
    .unwrap();
    assert_eq!(
        continuation,
        RuntimeCommand::ContinueAsNew {
            input: json!({ "cursor": "next-page" }),
        }
    );

    let child: RuntimeCommand = serde_json::from_value(json!({
        "type": "start_child_workflow",
        "child_id": "invoice",
        "spec": {
            "name": "invoice.child",
            "version": "1",
            "runtime": {
                "kind": "rust_embedded",
                "entrypoint": "tests::protocol",
                "export_name": "child"
            }
        },
        "input": { "invoice_id": 7 }
    }))
    .unwrap();
    assert!(matches!(
        child,
        RuntimeCommand::StartChildWorkflow {
            child_id,
            cancellation_policy: ChildWorkflowCancellationPolicy::RequestCancellation,
            ..
        } if child_id == "invoice"
    ));

    let batch: RuntimeCommand = serde_json::from_value(json!({
        "type": "start_child_workflows",
        "children": [
            {
                "child_id": "invoice-1",
                "spec": {
                    "name": "invoice.child",
                    "version": "1",
                    "runtime": {
                        "kind": "rust_embedded",
                        "entrypoint": "tests::protocol",
                        "export_name": "child"
                    }
                },
                "input": { "invoice_id": 1 }
            },
            {
                "child_id": "invoice-2",
                "spec": {
                    "name": "invoice.child",
                    "version": "1",
                    "runtime": {
                        "kind": "rust_embedded",
                        "entrypoint": "tests::protocol",
                        "export_name": "child"
                    }
                },
                "input": { "invoice_id": 2 },
                "cancellation_policy": "abandon"
            }
        ]
    }))
    .unwrap();
    assert_eq!(
        batch,
        RuntimeCommand::StartChildWorkflows {
            children: vec![
                ChildWorkflowCommand::new(
                    "invoice-1",
                    WorkflowSpec::rust_embedded("invoice.child", "1", "tests::protocol", "child",),
                    json!({ "invoice_id": 1 }),
                ),
                ChildWorkflowCommand::new(
                    "invoice-2",
                    WorkflowSpec::rust_embedded("invoice.child", "1", "tests::protocol", "child",),
                    json!({ "invoice_id": 2 }),
                )
                .with_cancellation_policy(ChildWorkflowCancellationPolicy::Abandon),
            ],
        }
    );
}

#[test]
fn run_summary_accepts_payloads_from_before_new_status_counters_were_added() {
    let summary: WorkflowRunSummary = serde_json::from_value(json!({
        "total_runs": 0,
        "pending_runs": 0,
        "running_runs": 0,
        "suspended_runs": 0,
        "completed_runs": 0,
        "failed_runs": 0,
        "cancelled_runs": 0,
        "terminal_runs": 0,
        "non_terminal_runs": 0,
        "open_waits": 0,
        "active_hooks": 0,
        "pending_retries": 0
    }))
    .unwrap();
    assert_eq!(summary.cancelling_runs, 0);
    assert_eq!(summary.continued_as_new_runs, 0);
    assert_eq!(summary.open_child_workflows, 0);
    assert_eq!(summary.open_signal_waits, 0);
}

#[test]
fn flow_event_envelope_uses_stable_native_history_field_names() {
    let envelope = FlowEventEnvelope::new(
        "run-1",
        7,
        Uuid::new_v4(),
        Utc::now(),
        FlowEvent::RunCancelled { reason: None },
    );

    let encoded = serde_json::to_value(envelope).unwrap();
    assert_eq!(encoded["run_id"], "run-1");
    assert_eq!(
        encoded["schema_version"],
        FLOW_EVENT_ENVELOPE_SCHEMA_VERSION
    );
    assert_eq!(encoded["sequence"], 7);
    assert!(encoded["event_id"].as_str().is_some());
    assert!(encoded["timestamp"].as_str().is_some());
    assert_eq!(encoded["event"]["type"], "run_cancelled");
    assert_eq!(encoded["event"]["reason"], serde_json::Value::Null);
    assert!(
        encoded.get("key").is_none(),
        "native runtime history envelopes should not include derived event keys"
    );
}

#[test]
fn flow_event_envelope_defaults_legacy_schema_version_and_rejects_future_versions() {
    let envelope = FlowEventEnvelope::new(
        "run-legacy",
        1,
        Uuid::new_v4(),
        Utc::now(),
        FlowEvent::RunStarted,
    );
    let mut legacy = serde_json::to_value(&envelope).unwrap();
    legacy
        .as_object_mut()
        .expect("envelope is an object")
        .remove("schema_version");
    let decoded: FlowEventEnvelope = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.schema_version, FLOW_EVENT_ENVELOPE_SCHEMA_VERSION);
    decoded.validate_schema_version().unwrap();

    let mut future = envelope;
    future.schema_version = FLOW_EVENT_ENVELOPE_SCHEMA_VERSION + 1;
    let error = future.validate_schema_version().unwrap_err();
    assert!(matches!(
        error,
        FlowError::UnsupportedEventSchemaVersion {
            version,
            supported
        } if version == FLOW_EVENT_ENVELOPE_SCHEMA_VERSION + 1
            && supported == FLOW_EVENT_ENVELOPE_SCHEMA_VERSION
    ));
}

#[test]
fn step_invocation_exposes_a_stable_attempt_idempotency_key() {
    let history = vec![
        FlowEventEnvelope::new(
            "run/1",
            1,
            Uuid::new_v4(),
            Utc::now(),
            FlowEvent::RunCreated {
                spec: WorkflowSpec::rust_embedded("example", "1", "tests::example", "main"),
                input: json!({}),
            },
        ),
        FlowEventEnvelope::new(
            "run/1",
            2,
            Uuid::new_v4(),
            Utc::now(),
            FlowEvent::StepStarted {
                step_id: "step/1".to_string(),
                attempt: 3,
            },
        ),
    ];
    let invocation = StepInvocation::new(
        "run/1",
        "step/1",
        "sendEmail",
        json!({ "to": "user@example.com" }),
        history,
    );
    assert_eq!(invocation.attempt, 3);
    assert_eq!(invocation.idempotency_key, "flow.step.v1/5/run/16:step/1/3");
    let encoded = serde_json::to_value(invocation).unwrap();
    assert_eq!(encoded["attempt"], 3);
    assert_eq!(encoded["idempotency_key"], "flow.step.v1/5/run/16:step/1/3");
}

#[test]
fn continue_as_new_event_uses_stable_wire_shape_and_event_key() {
    let event = FlowEvent::RunContinuedAsNew {
        successor_run_id: "successor-1".to_string(),
        input: json!({ "cursor": 2 }),
    };
    assert_eq!(event.event_key(), "flow.run.continued_as_new");
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        json!({
            "type": "run_continued_as_new",
            "successor_run_id": "successor-1",
            "input": { "cursor": 2 }
        })
    );
}

#[test]
fn cancelled_step_event_uses_stable_wire_shape_and_event_key() {
    let event = FlowEvent::StepCancelled {
        step_id: "slow-sibling".to_string(),
        attempt: 2,
        reason: "outcome is unknown after a concurrent batch abort".to_string(),
    };
    assert_eq!(event.event_key(), "flow.step.cancelled");
    assert_eq!(
        serde_json::to_value(event).unwrap(),
        json!({
            "type": "step_cancelled",
            "step_id": "slow-sibling",
            "attempt": 2,
            "reason": "outcome is unknown after a concurrent batch abort"
        })
    );
}

#[test]
fn child_workflow_events_use_stable_wire_shapes_and_event_keys() {
    let requested = FlowEvent::ChildWorkflowRequested {
        child_id: "invoice".into(),
        child_run_id: "child-run-1".into(),
        spec: a3s_flow::WorkflowSpec::rust_embedded(
            "invoice.child",
            "1",
            "tests::protocol",
            "child",
        ),
        input: json!({ "invoice_id": 7 }),
        cancellation_policy: ChildWorkflowCancellationPolicy::RequestCancellation,
    };
    assert_eq!(requested.event_key(), "flow.child.workflow.requested");
    let requested_json = serde_json::to_value(requested).unwrap();
    assert_eq!(requested_json["type"], "child_workflow_requested");
    assert_eq!(requested_json["child_run_id"], "child-run-1");
    assert_eq!(
        requested_json["cancellation_policy"],
        "request_cancellation"
    );

    let resolved = FlowEvent::ChildWorkflowResolved {
        child_id: "invoice".into(),
        outcome: WorkflowTerminalOutcome::Completed {
            output: json!({ "accepted": true }),
        },
    };
    assert_eq!(resolved.event_key(), "flow.child.workflow.resolved");
    assert_eq!(
        serde_json::to_value(resolved).unwrap(),
        json!({
            "type": "child_workflow_resolved",
            "child_id": "invoice",
            "outcome": {
                "type": "completed",
                "output": { "accepted": true }
            }
        })
    );
}

#[test]
fn signal_contracts_commands_and_events_use_stable_wire_shapes() {
    let legacy_spec: WorkflowSpec = serde_json::from_value(json!({
        "name": "legacy.workflow",
        "version": "1",
        "runtime": {
            "kind": "rust_embedded",
            "entrypoint": "tests::protocol",
            "export_name": "main"
        }
    }))
    .unwrap();
    assert!(legacy_spec.signal_names.is_empty());

    let declared = WorkflowSpec::rust_embedded("signal.workflow", "1", "tests::protocol", "main")
        .with_signal("order.released")
        .with_signal("order.approved");
    assert_eq!(
        serde_json::to_value(declared).unwrap()["signal_names"],
        json!(["order.approved", "order.released"])
    );

    let command = RuntimeCommand::WaitForSignal {
        wait_id: "approval".to_string(),
        signal_name: "order.approved".to_string(),
    };
    assert_eq!(
        serde_json::to_value(command).unwrap(),
        json!({
            "type": "wait_for_signal",
            "wait_id": "approval",
            "signal_name": "order.approved"
        })
    );

    let received = FlowEvent::SignalReceived {
        signal: WorkflowSignal::new("delivery-1", "order.approved", json!({ "approved": true })),
    };
    assert_eq!(received.event_key(), "flow.signal.received");
    assert_eq!(
        serde_json::to_value(received).unwrap(),
        json!({
            "type": "signal_received",
            "signal": {
                "signal_id": "delivery-1",
                "name": "order.approved",
                "payload": { "approved": true }
            }
        })
    );
    assert_eq!(
        FlowEvent::SignalWaitCreated {
            wait_id: "approval".to_string(),
            signal_name: "order.approved".to_string(),
        }
        .event_key(),
        "flow.signal.wait.created"
    );
    assert_eq!(
        FlowEvent::SignalWaitCompleted {
            wait_id: "approval".to_string(),
            signal_id: "delivery-1".to_string(),
        }
        .event_key(),
        "flow.signal.wait.completed"
    );
}

#[test]
fn native_ts_authoring_types_track_runtime_protocol_shape() {
    let types = include_str!("../examples/native-ts/a3s-flow-runtime.d.ts");
    for event_type in [
        "run_created",
        "run_started",
        "run_completed",
        "run_failed",
        "run_cancellation_requested",
        "run_cancelled",
        "run_timed_out",
        "run_retry_exhausted",
        "run_host_shutdown",
        "run_continued_as_new",
        "run_progress_recorded",
        "child_operation_linked",
        "child_workflow_requested",
        "child_workflow_resolved",
        "signal_received",
        "signal_wait_created",
        "signal_wait_completed",
        "step_created",
        "step_started",
        "step_completed",
        "step_retrying",
        "step_failed",
        "step_non_retryable",
        "step_cancelled",
        "wait_created",
        "wait_completed",
        "hook_created",
        "hook_received",
        "hook_disposed",
    ] {
        assert!(
            types.contains(&format!("\"{event_type}\"")),
            "missing FlowEvent type {event_type} in native TypeScript authoring contract"
        );
    }

    assert!(types.contains("event_id: string;"));
    assert!(!types.contains("\n  key: string;"));
    assert!(types.contains("runtime_build_id?: string;"));
    assert!(types.contains("patch_markers?: string[];"));
    assert!(types.contains("signal_names?: string[];"));
    assert!(types.contains("token: string;"));
    assert!(types.contains("retry?: RetryPolicy;"));
    assert!(types.contains("backoff?: \"fixed\" | \"exponential\";"));
    assert!(types.contains("max_delay_ms?: number;"));
    assert!(types.contains("retry_after: string | null;"));
    assert!(types.contains("attempt: number;"));
    assert!(types.contains("idempotency_key: string;"));
    assert!(types.contains("type: \"record_progress\"; progress: WorkflowProgress"));
    assert!(types.contains("type: \"link_child_operation\"; child: ChildOperationReference"));
    assert!(types.contains("type: \"start_child_workflow\";"));
    assert!(types.contains("type: \"start_child_workflows\";"));
    assert!(types.contains("children: ChildWorkflowCommand[];"));
    assert!(types.contains("type: \"wait_for_signal\";"));
    assert!(types.contains("type: \"cancel\""));
    assert!(types.contains("type: \"continue_as_new\"; input: Json"));
    assert!(!types.contains("export function stepOutput"));
}
