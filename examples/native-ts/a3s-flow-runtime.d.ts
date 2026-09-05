export type Json =
  | null
  | boolean
  | number
  | string
  | Json[]
  | { [key: string]: Json };

export type NativeRuntimeKind = "workflow" | "step";

export type RuntimeFamily = "native_ts" | "rust_embedded";

export type RuntimeSpec = {
  kind: RuntimeFamily;
  entrypoint: string;
  export_name: string;
};

export type WorkflowSpec = {
  name: string;
  version: string;
  runtime: RuntimeSpec;
  runtime_build_id?: string;
  patch_markers?: string[];
  signal_names?: string[];
};

export type WorkflowSignal = {
  signal_id: string;
  name: string;
  payload: Json;
};

export type RetryPolicy = {
  max_attempts: number;
  delay_ms: number;
  backoff?: "fixed" | "exponential";
  max_delay_ms?: number;
  on_exhausted?: "fail_run" | "continue_workflow";
};

export type CancellationRequest = {
  reason?: string | null;
};

export type WorkflowProgress = {
  progress_id: string;
  completed: number;
  total?: number;
  message?: string;
  details?: Json;
};

export type ChildOperationReference = {
  reference_id: string;
  kind: string;
  operation_id: string;
  flow_run_id?: string;
  metadata?: Json;
};

export type ChildWorkflowCancellationPolicy =
  | "request_cancellation"
  | "abandon";

export type ChildWorkflowCommand = {
  child_id: string;
  spec: WorkflowSpec;
  input: Json;
  cancellation_policy?: ChildWorkflowCancellationPolicy;
};

export type WorkflowTerminalOutcome =
  | { type: "completed"; output: Json }
  | { type: "failed"; error: string }
  | { type: "cancelled"; reason?: string | null }
  | { type: "timed_out"; deadline: string; reason?: string | null }
  | { type: "retry_exhausted"; step_id: string; attempt: number; error: string }
  | { type: "host_shutdown"; reason?: string | null }
  | { type: "continued_as_new"; successor_run_id: string };

export type RuntimeCommand =
  | { type: "complete"; output: Json }
  | { type: "fail"; error: string }
  | { type: "cancel" }
  | { type: "timeout"; deadline: string; reason: string | null }
  | { type: "continue_as_new"; input: Json }
  | { type: "record_progress"; progress: WorkflowProgress }
  | { type: "link_child_operation"; child: ChildOperationReference }
  | {
      type: "start_child_workflow";
      child_id: string;
      spec: WorkflowSpec;
      input: Json;
      cancellation_policy?: ChildWorkflowCancellationPolicy;
    }
  | {
      type: "start_child_workflows";
      children: ChildWorkflowCommand[];
    }
  | {
      type: "schedule_step";
      step_id: string;
      step_name: string;
      input: Json;
      retry?: RetryPolicy;
    }
  | {
      type: "schedule_steps";
      steps: StepCommand[];
    }
  | {
      type: "wait_until";
      wait_id: string;
      resume_at: string;
    }
  | {
      type: "create_hook";
      hook_id: string;
      token: string;
      metadata: Json;
    }
  | {
      type: "wait_for_signal";
      wait_id: string;
      signal_name: string;
    };

export type StepCommand = {
  step_id: string;
  step_name: string;
  input: Json;
  retry?: RetryPolicy;
};

export type FlowEvent =
  | {
      type: "run_created";
      spec: WorkflowSpec;
      input: Json;
    }
  | { type: "run_started" }
  | { type: "run_completed"; output: Json }
  | { type: "run_failed"; error: string }
  | { type: "run_cancellation_requested"; request: CancellationRequest }
  | { type: "run_cancelled"; reason: string | null }
  | { type: "run_timed_out"; deadline: string; reason: string | null }
  | {
      type: "run_retry_exhausted";
      step_id: string;
      attempt: number;
      error: string;
    }
  | { type: "run_host_shutdown"; reason: string | null }
  | { type: "run_continued_as_new"; successor_run_id: string; input: Json }
  | { type: "run_progress_recorded"; progress: WorkflowProgress }
  | { type: "child_operation_linked"; child: ChildOperationReference }
  | {
      type: "child_workflow_requested";
      child_id: string;
      child_run_id: string;
      spec: WorkflowSpec;
      input: Json;
      cancellation_policy: ChildWorkflowCancellationPolicy;
    }
  | {
      type: "child_workflow_resolved";
      child_id: string;
      outcome: WorkflowTerminalOutcome;
    }
  | { type: "signal_received"; signal: WorkflowSignal }
  | {
      type: "signal_wait_created";
      wait_id: string;
      signal_name: string;
    }
  | {
      type: "signal_wait_completed";
      wait_id: string;
      signal_id: string;
    }
  | {
      type: "step_created";
      step_id: string;
      step_name: string;
      input: Json;
      retry: RetryPolicy;
    }
  | { type: "step_started"; step_id: string; attempt: number }
  | { type: "step_completed"; step_id: string; output: Json }
  | {
      type: "step_retrying";
      step_id: string;
      attempt: number;
      error: string;
      retry_after: string | null;
    }
  | { type: "step_failed"; step_id: string; attempt: number; error: string }
  | {
      type: "step_cancelled";
      step_id: string;
      attempt: number;
      reason: string;
    }
  | { type: "wait_created"; wait_id: string; resume_at: string }
  | { type: "wait_completed"; wait_id: string }
  | { type: "hook_created"; hook_id: string; token: string; metadata: Json }
  | { type: "hook_received"; hook_id: string; payload: Json }
  | { type: "hook_disposed"; hook_id: string };

export type FlowEventEnvelope = {
  schema_version?: number;
  event_id: string;
  run_id: string;
  sequence: number;
  timestamp: string;
  event: FlowEvent;
};

export type WorkflowInvocation<Input extends Json = Json> = {
  run_id: string;
  spec: WorkflowSpec;
  input: Input;
  history: FlowEventEnvelope[];
};

export type StepInvocation<Input extends Json = Json> = {
  run_id: string;
  step_id: string;
  attempt: number;
  step_name: string;
  input: Input;
  history: FlowEventEnvelope[];
  idempotency_key: string;
};

export type StepDefinition<Input extends Json = Json, Output extends Json = Json> = (
  invocation: StepInvocation<Input>,
) => Output | Promise<Output>;

export type NativeRuntimeRequest<Payload extends Json | object = Json> = {
  protocol: "a3s.flow.native_ts.v1";
  kind: NativeRuntimeKind;
  exportName: string;
  sourceHash: string;
  payload: Payload;
};

export type NativeRuntimeResponse<Output extends Json = Json> =
  | {
      protocol: "a3s.flow.native_ts.v1";
      kind: NativeRuntimeKind;
      ok: true;
      output: Output;
    }
  | {
      protocol: "a3s.flow.native_ts.v1";
      kind: NativeRuntimeKind;
      ok: false;
      error: string;
    };
