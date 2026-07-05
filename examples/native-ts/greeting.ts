import type {
  FlowEventEnvelope,
  RuntimeCommand,
  StepInvocation,
  WorkflowInvocation,
} from "./a3s-flow-runtime";

type GreetingInput = {
  name: string;
};

type GreetingStepInput = {
  name: string;
};

type GreetingOutput = {
  message: string;
};

function completedStep<Output>(
  history: FlowEventEnvelope[],
  stepId: string,
): Output | undefined {
  const event = history.find(
    (item) => item.event.type === "step_completed" && item.event.step_id === stepId,
  );
  return event?.event.type === "step_completed"
    ? (event.event.output as Output)
    : undefined;
}

export async function main(
  invocation: WorkflowInvocation<GreetingInput>,
): Promise<RuntimeCommand> {
  const greeting = completedStep<GreetingOutput>(invocation.history, "greet");
  if (greeting) {
    return { type: "complete", output: greeting };
  }

  return {
    type: "schedule_step",
    step_id: "greet",
    step_name: "greet_user",
    input: { name: invocation.input.name },
    retry: { max_attempts: 3, delay_ms: 0 },
  };
}

export const steps = {
  async greet_user(
    invocation: StepInvocation<GreetingStepInput>,
  ): Promise<GreetingOutput> {
    return { message: `hello ${invocation.input.name}` };
  },
};

