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
};

export type RetryPolicy = {
  max_attempts: number;
  delay_ms: number;
};

export type RuntimeCommand =
  | { type: "complete"; output: Json }
  | { type: "fail"; error: string }
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
      token?: string;
      metadata: Json;
    };

export type StepCommand = {
  step_id: string;
  step_name: string;
  input: Json;
  retry: RetryPolicy;
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
      retry_after?: string;
    }
  | { type: "step_failed"; step_id: string; attempt: number; error: string }
  | { type: "wait_created"; wait_id: string; resume_at: string }
  | { type: "wait_completed"; wait_id: string }
  | { type: "hook_created"; hook_id: string; token?: string; metadata: Json }
  | { type: "hook_received"; hook_id: string; payload: Json };

export type FlowEventEnvelope = {
  event_id: string;
  run_id: string;
  sequence: number;
  key: string;
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
  step_name: string;
  input: Input;
  history: FlowEventEnvelope[];
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

export function stepOutput<Output extends Json>(
  history: FlowEventEnvelope[],
  stepId: string,
): Output | undefined;

