use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, HookCallbackRoute, HookMetadata, RuntimeCommand,
    StepInvocation, WorkflowInvocation, WorkflowRunStatus, WorkflowSpec,
};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

fn spec() -> WorkflowSpec {
    WorkflowSpec::rust_embedded("context.workflow", "0.1.0", "tests::context", "main")
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
struct User {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct Approval {
    approved: bool,
}

#[derive(Debug, Deserialize, PartialEq)]
struct FinalOutput {
    user: User,
    approved: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextInput {
    user_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadUserInput {
    user_id: String,
}

struct ContextRuntime;

#[async_trait]
impl FlowRuntime for ContextRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();
        let input = ctx.input_as::<ContextInput>()?;

        let Some(user) = ctx.step_output_as::<User>("load-user")? else {
            return Ok(ctx.schedule_step(
                "load-user",
                "loadUser",
                json!({ "userId": input.user_id }),
            ));
        };

        if !ctx.wait_completed("review-window") {
            return Ok(ctx.wait_until("review-window", Utc::now()));
        }

        let Some(approval) = ctx.hook_payload_as::<Approval>("approval")? else {
            let metadata = HookMetadata::human_approval(format!("user:{}", user.id))
                .with_callback_route(HookCallbackRoute::post("/callbacks/flow/hooks/{token}"))
                .with_label("source", "context-test")
                .with_data("user", json!(user.name));
            return Ok(ctx.create_hook_with_metadata("approval", "approval-token", metadata)?);
        };

        Ok(ctx.complete(json!({
            "user": user,
            "approved": approval.approved,
        })))
    }

    async fn run_step(&self, invocation: StepInvocation) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "loadUser" => {
                let input = invocation.input_as::<LoadUserInput>()?;
                Ok(json!({
                    "id": input.user_id,
                    "name": "Ada",
                }))
            }
            other => Err(FlowError::Runtime(format!("unknown step {other}"))),
        }
    }
}

#[tokio::test]
async fn workflow_context_drives_step_wait_and_hook_flow() {
    let engine = FlowEngine::in_memory(Arc::new(ContextRuntime));
    let run_id = engine
        .start(spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();

    let waiting = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(waiting.status, WorkflowRunStatus::Suspended);
    assert_eq!(
        waiting.steps["load-user"].output.as_ref().unwrap()["name"],
        "Ada"
    );
    assert!(waiting.hooks.is_empty());

    engine.resume_wait(&run_id, "review-window").await.unwrap();
    let hooked = engine.snapshot(&run_id).await.unwrap();
    assert_eq!(hooked.status, WorkflowRunStatus::Suspended);
    let metadata = &hooked.hooks["approval"].metadata;
    assert_eq!(metadata["kind"], "human_approval");
    assert_eq!(metadata["subject"], "user:u1");
    assert_eq!(metadata["callback"]["method"], "POST");
    assert_eq!(
        metadata["callback"]["path"],
        "/callbacks/flow/hooks/{token}"
    );
    assert_eq!(metadata["labels"]["source"], "context-test");
    assert_eq!(metadata["data"]["user"], "Ada");

    engine
        .resume_hook(&run_id, "approval", json!({ "approved": true }))
        .await
        .unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();

    assert_eq!(completed.status, WorkflowRunStatus::Completed);
    let input = completed.input_as::<ContextInput>().unwrap();
    assert_eq!(input.user_id, "u1");
    let user = completed.step_output_as::<User>("load-user").unwrap();
    assert_eq!(
        user,
        Some(User {
            id: "u1".to_string(),
            name: "Ada".to_string(),
        })
    );
    let approval = completed
        .hook_payload_as::<Approval>("approval")
        .unwrap()
        .expect("approval payload");
    assert!(approval.approved);
    let output = completed
        .output_as::<FinalOutput>()
        .unwrap()
        .expect("terminal output");
    assert_eq!(
        output,
        FinalOutput {
            user: User {
                id: "u1".to_string(),
                name: "Ada".to_string(),
            },
            approved: true,
        }
    );

    let step = completed.steps.get("load-user").expect("step snapshot");
    assert_eq!(
        step.output_as::<User>().unwrap().unwrap().name,
        "Ada".to_string()
    );
    let hook = completed.hooks.get("approval").expect("hook snapshot");
    assert!(hook.payload_as::<Approval>().unwrap().unwrap().approved);
    assert!(completed
        .step_output_as::<User>("missing-step")
        .unwrap()
        .is_none());
    assert!(completed
        .hook_payload_as::<Approval>("missing-hook")
        .unwrap()
        .is_none());
}

#[test]
fn typed_input_helpers_surface_serialization_errors() {
    let invocation = WorkflowInvocation {
        run_id: "typed-input-error".to_string(),
        spec: spec(),
        input: json!({ "userId": 42 }),
        history: Vec::new(),
    };
    let workflow_error = invocation.context().input_as::<ContextInput>().unwrap_err();
    assert!(matches!(workflow_error, FlowError::Serialization(_)));

    let step = StepInvocation {
        run_id: "typed-input-error".to_string(),
        step_id: "load-user".to_string(),
        step_name: "loadUser".to_string(),
        input: json!({ "userId": 42 }),
        history: Vec::new(),
    };
    let step_error = step.input_as::<LoadUserInput>().unwrap_err();
    assert!(matches!(step_error, FlowError::Serialization(_)));
}

#[tokio::test]
async fn snapshot_typed_helpers_surface_serialization_errors() {
    let engine = FlowEngine::in_memory(Arc::new(ContextRuntime));
    let run_id = engine
        .start(spec(), json!({ "userId": "u1" }))
        .await
        .unwrap();
    engine.resume_wait(&run_id, "review-window").await.unwrap();
    engine
        .resume_hook(&run_id, "approval", json!({ "approved": true }))
        .await
        .unwrap();
    let completed = engine.snapshot(&run_id).await.unwrap();

    let input_error = completed.input_as::<Approval>().unwrap_err();
    assert!(matches!(input_error, FlowError::Serialization(_)));
    let output_error = completed.output_as::<User>().unwrap_err();
    assert!(matches!(output_error, FlowError::Serialization(_)));
    let step_error = completed
        .step_output_as::<Approval>("load-user")
        .unwrap_err();
    assert!(matches!(step_error, FlowError::Serialization(_)));
    let hook_error = completed.hook_payload_as::<User>("approval").unwrap_err();
    assert!(matches!(hook_error, FlowError::Serialization(_)));
}
