import type { JsonObject } from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import {
  HOST_ITERATION_MAX_ITEMS,
  HOST_LOOP_MAX_ROUNDS,
  HOST_STEP_TIMEOUT_SECS,
  hostRunSettings,
  hostScopeOf,
  hostScopeOptions,
  hostTimeoutFallbackOptions,
  hostTimeoutInRange,
  hostWithRunSettings,
  hostWithScopeSettings,
  type HostScopeSettings,
} from './WorkflowPlayground.host-scopes';
import {
  hostApprovalInputs,
  hostConditionChain,
  hostConditionInput,
  hostOtherwiseTargets,
  hostStepEditorKind,
  hostStepOutputOptions,
} from './WorkflowPlayground.host';
import type {
  PlaygroundGraphState,
  PlaygroundNode,
} from './WorkflowPlayground.model';

export type WorkflowPlaygroundHostStepEditorProps = {
  graph: PlaygroundGraphState;
  locale: FlowWebsiteLocale;
  node: PlaygroundNode;
  onApprovalInputs: (nodeId: string, inputs: string[]) => void;
  onConditionInput: (nodeId: string, source: string) => void;
  onOtherwise: (nodeId: string, targets: string[]) => void;
  /** Change the plan steps and redraw (run settings, iteration / loop settings). */
  onPlanEdit: (edit: (steps: JsonObject[]) => JsonObject[]) => void;
  /** Tell the operator why an entry was not applied. */
  onRefuse: (message: string) => void;
};

/** The plan steps the canvas carries now. */
function canvasSteps(graph: PlaygroundGraphState): JsonObject[] {
  return graph.nodes
    .map((node) => node.data.hostPlanStep as JsonObject | undefined)
    .filter((step): step is JsonObject => Boolean(step));
}

/** A whole number from a text field, or null when it is empty. */
function wholeNumber(value: string): number | null | undefined {
  const text = value.trim();
  if (!text) return null;
  return /^\d+$/.test(text) ? Number(text) : undefined;
}

function toggled(list: readonly string[], item: string, on: boolean): string[] {
  return on
    ? [...list.filter((entry) => entry !== item), item]
    : list.filter((entry) => entry !== item);
}

type SettingsProps = {
  locale: FlowWebsiteLocale;
  steps: JsonObject[];
  stepId: string;
  onPlanEdit: WorkflowPlaygroundHostStepEditorProps['onPlanEdit'];
  onRefuse: WorkflowPlaygroundHostStepEditorProps['onRefuse'];
};

/** An agent step's own deadline, timeout fallbacks and list output. */
function AgentRunSettings({
  locale,
  steps,
  stepId,
  onPlanEdit,
  onRefuse,
}: SettingsProps) {
  const zh = locale === 'zh';
  const step = steps.find((item) => item.step_id === stepId);
  if (!step) return null;
  const settings = hostRunSettings(step);
  const inBody = Boolean(hostScopeOf(steps, stepId));
  const fallbacks = hostTimeoutFallbackOptions(steps, stepId);
  const outputs = Array.isArray(step.outputs) ? step.outputs : [];
  const apply = (next: typeof settings) =>
    onPlanEdit((current) => hostWithRunSettings(current, stepId, next));
  const commitTimeout = (value: string) => {
    const secs = wholeNumber(value);
    if (secs === settings.timeoutSecs) return;
    if (secs === undefined || (secs !== null && !hostTimeoutInRange(secs))) {
      onRefuse(
        zh
          ? `超时要填 ${HOST_STEP_TIMEOUT_SECS.min}–${HOST_STEP_TIMEOUT_SECS.max} 之间的整数秒，留空表示用统一超时。`
          : `A timeout is a whole number of seconds from ${HOST_STEP_TIMEOUT_SECS.min} to ${HOST_STEP_TIMEOUT_SECS.max}; leave it empty for the host-wide one.`,
      );
      return;
    }
    apply({
      ...settings,
      timeoutSecs: secs,
      onTimeout: secs === null ? [] : settings.onTimeout,
    });
  };
  return (
    <section
      className="flow-playground-host-step-editor"
      data-testid="host-step-editor"
    >
      <h3>{zh ? '运行设置' : 'Run settings'}</h3>
      <label className="flow-playground-host-step-editor__field">
        <span>
          {zh
            ? `超时（秒，${HOST_STEP_TIMEOUT_SECS.min}–${HOST_STEP_TIMEOUT_SECS.max}；留空用统一超时）`
            : `Timeout (seconds, ${HOST_STEP_TIMEOUT_SECS.min}–${HOST_STEP_TIMEOUT_SECS.max}; empty: host-wide)`}
        </span>
        <input
          data-testid="host-timeout-secs"
          defaultValue={settings.timeoutSecs ?? ''}
          inputMode="numeric"
          key={`${stepId}:${settings.timeoutSecs ?? ''}`}
          onBlur={(event) => commitTimeout(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') commitTimeout(event.currentTarget.value);
          }}
        />
      </label>
      {inBody ? (
        <p>
          {zh
            ? '循环体里的步骤超时后不能改跑别的步骤。'
            : 'A step inside an iteration or loop takes no timeout fallback.'}
        </p>
      ) : (
        <fieldset
          className="flow-playground-host-step-editor__group"
          disabled={settings.timeoutSecs === null}
        >
          <legend>
            {zh
              ? '超时后改为运行（不选则转人工处理）'
              : 'On timeout, run instead (none: hand over to a human)'}
          </legend>
          {fallbacks.map((target) => (
            <label
              key={target}
              className="flow-playground-host-step-editor__option"
            >
              <input
                checked={settings.onTimeout.includes(target)}
                data-testid={`host-on-timeout-${target}`}
                onChange={(event) =>
                  apply({
                    ...settings,
                    onTimeout: toggled(
                      settings.onTimeout,
                      target,
                      event.currentTarget.checked,
                    ),
                  })
                }
                type="checkbox"
              />
              <code>{target}</code>
            </label>
          ))}
        </fieldset>
      )}
      <label className="flow-playground-host-step-editor__option">
        <input
          checked={settings.listOutput}
          data-testid="host-list-output"
          disabled={outputs.length !== 1}
          onChange={(event) =>
            apply({ ...settings, listOutput: event.currentTarget.checked })
          }
          type="checkbox"
        />
        <span>
          {zh
            ? '输出是一个列表（JSON 数组，可以逐项处理；只有一个输出时可选）'
            : 'Its output is a list (a JSON array an iteration can go over; one output only)'}
        </span>
      </label>
    </section>
  );
}

/** An iteration's / loop's list, body, collected output, check and bound. */
function ScopeSettings({
  locale,
  steps,
  stepId,
  onPlanEdit,
  onRefuse,
}: SettingsProps) {
  const zh = locale === 'zh';
  const scope = steps.find((item) => item.step_id === stepId);
  if (!scope) return null;
  const loop = scope.kind === 'loop';
  const options = hostScopeOptions(steps, stepId);
  const body = Array.isArray(scope.body) ? scope.body.map(String) : [];
  const input =
    Array.isArray(scope.inputs) && typeof scope.inputs[0] === 'string'
      ? scope.inputs[0]
      : '';
  const collect = typeof scope.collect === 'string' ? scope.collect : '';
  const until = typeof scope.until === 'string' ? scope.until : '';
  const passLabel =
    typeof scope.pass_label === 'string' ? scope.pass_label : '';
  const bound = loop ? scope.max_rounds : scope.max_items;
  const max = loop ? HOST_LOOP_MAX_ROUNDS : HOST_ITERATION_MAX_ITEMS;
  const apply = (patch: HostScopeSettings) =>
    onPlanEdit((current) => hostWithScopeSettings(current, stepId, patch));
  // A required choice that is not made yet: a save would be refused.
  const outputSelect = (
    testId: string,
    value: string,
    choices: string[],
    onChange: (next: string) => void,
  ) => (
    <select
      aria-invalid={!value}
      data-testid={testId}
      onChange={(event) => onChange(event.currentTarget.value)}
      style={
        value
          ? undefined
          : { outline: '1px solid var(--workflow-danger, #c0392b)' }
      }
      value={value}
    >
      <option value="">{zh ? '（未选择）' : '(none)'}</option>
      {choices.map((choice) => (
        <option key={choice} value={choice}>
          {choice}
        </option>
      ))}
    </select>
  );
  const commitBound = (value: string) => {
    const next = wholeNumber(value);
    if (next === bound) return;
    if (next === null || next === undefined || next < 1 || next > max) {
      onRefuse(
        zh
          ? `要填 1–${max} 之间的整数。`
          : `Enter a whole number from 1 to ${max}.`,
      );
      return;
    }
    apply(loop ? { maxRounds: next } : { maxItems: next });
  };
  const commitLabel = (value: string) => {
    if (value === passLabel) return;
    if (!value.trim() || value.trim() !== value || value.length > 64) {
      onRefuse(
        zh
          ? '通过标签要 1–64 个字符，前后不能有空格。'
          : 'A pass label is 1 to 64 characters without surrounding spaces.',
      );
      return;
    }
    apply({ passLabel: value });
  };
  return (
    <section
      className="flow-playground-host-step-editor"
      data-testid="host-step-editor"
    >
      <h3>{loop ? (zh ? '循环' : 'Loop') : zh ? '逐项处理' : 'Iteration'}</h3>
      {loop ? (
        <label className="flow-playground-host-step-editor__field">
          <span>
            {zh
              ? '每轮读哪个上游输出（循环体里读它的步骤会一起改）'
              : 'Which upstream output every round reads (body steps reading it follow)'}
          </span>
          {outputSelect('host-loop-input', input, options.upstreams, (next) =>
            apply({ input: next }),
          )}
        </label>
      ) : (
        <label className="flow-playground-host-step-editor__field">
          <span>
            {zh
              ? '逐项处理哪个列表（某一步勾选了“输出是列表”）'
              : 'Goes over which list (a step whose output is a list)'}
          </span>
          {outputSelect('host-iteration-input', input, options.lists, (next) =>
            apply({ input: next }),
          )}
        </label>
      )}
      <fieldset className="flow-playground-host-step-editor__group">
        <legend>
          {loop
            ? zh
              ? `每轮运行的步骤（最多 8 个）`
              : 'Steps each round runs (up to 8)'
            : zh
              ? `每一项运行的步骤（最多 8 个）`
              : 'Steps each item runs (up to 8)'}
        </legend>
        {options.bodyCandidates.map((candidate) => (
          <label
            key={candidate}
            className="flow-playground-host-step-editor__option"
          >
            <input
              checked={body.includes(candidate)}
              data-testid={`host-scope-body-${candidate}`}
              disabled={options.lockedMembers.includes(candidate)}
              title={
                options.lockedMembers.includes(candidate)
                  ? zh
                    ? '它读了循环体里另一步的输出，只能留在循环体里'
                    : "It reads another body step's output, so it stays in the body"
                  : undefined
              }
              onChange={(event) => {
                const next = toggled(
                  body,
                  candidate,
                  event.currentTarget.checked,
                );
                if (next.length === 0 || next.length > 8) {
                  onRefuse(
                    zh
                      ? '至少要有 1 个、最多 8 个步骤。'
                      : 'Keep 1 to 8 steps.',
                  );
                  return;
                }
                apply({ body: next });
              }}
              type="checkbox"
            />
            <code>{candidate}</code>
          </label>
        ))}
      </fieldset>
      {loop ? (
        <>
          <label className="flow-playground-host-step-editor__field">
            <span>
              {zh
                ? '用哪个输出判断是否通过（首行等于通过标签即停止）'
                : 'Which output decides a round passed (its first line equals the pass label)'}
            </span>
            {outputSelect(
              'host-loop-until',
              until,
              options.bodyOutputs,
              (next) => apply({ until: next }),
            )}
          </label>
          <label className="flow-playground-host-step-editor__field">
            <span>{zh ? '通过标签' : 'Pass label'}</span>
            <input
              data-testid="host-loop-pass-label"
              defaultValue={passLabel}
              key={`${stepId}:${passLabel}`}
              onBlur={(event) => commitLabel(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter')
                  commitLabel(event.currentTarget.value);
              }}
            />
          </label>
        </>
      ) : null}
      <label className="flow-playground-host-step-editor__field">
        <span>
          {loop
            ? zh
              ? '最终结果取哪个输出'
              : 'Which output is the result'
            : zh
              ? '每一项收集哪个输出'
              : 'Which output each item contributes'}
        </span>
        {outputSelect(
          'host-scope-collect',
          collect,
          options.bodyOutputs,
          (next) => apply({ collect: next }),
        )}
      </label>
      <label className="flow-playground-host-step-editor__field">
        <span>
          {loop
            ? zh
              ? `最多几轮（1–${max}）`
              : `At most rounds (1–${max})`
            : zh
              ? `最多几项（1–${max}）`
              : `At most items (1–${max})`}
        </span>
        <input
          data-testid="host-scope-bound"
          defaultValue={typeof bound === 'number' ? bound : ''}
          inputMode="numeric"
          key={`${stepId}:${String(bound)}`}
          onBlur={(event) => commitBound(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') commitBound(event.currentTarget.value);
          }}
        />
      </label>
    </section>
  );
}

function stepTitle(node: PlaygroundNode): string {
  const data = node.data.dagNode?.data as { title?: unknown } | undefined;
  return typeof data?.title === 'string' && data.title.trim()
    ? `${data.title} (${node.id})`
    : node.id;
}

/**
 * Host-mode settings of a control-flow plan step, above the node's generic
 * Flow settings: what a condition routes on and which steps its `otherwise`
 * branch runs (rewiring the last case node's `otherwise` port), and which
 * upstream outputs an approval gate shows its reviewer.  Case labels are the
 * condition nodes' own "True branch label"; a case's targets are its
 * `matched` wiring.  Saved through hostCanvasFromGraph like every canvas edit.
 */
export function WorkflowPlaygroundHostStepEditor({
  graph,
  locale,
  node,
  onApprovalInputs,
  onConditionInput,
  onOtherwise,
  onPlanEdit,
  onRefuse,
}: WorkflowPlaygroundHostStepEditorProps) {
  const kind = hostStepEditorKind(node);
  if (!kind) return null;
  const zh = locale === 'zh';
  if (kind === 'agent' || kind === 'iteration' || kind === 'loop') {
    const Settings = kind === 'agent' ? AgentRunSettings : ScopeSettings;
    return (
      <Settings
        locale={locale}
        onPlanEdit={onPlanEdit}
        onRefuse={onRefuse}
        stepId={node.id}
        steps={canvasSteps(graph)}
      />
    );
  }
  const outputs = hostStepOutputOptions(graph);
  if (kind === 'approval') {
    const selected = new Set(hostApprovalInputs(graph, node.id));
    return (
      <section
        className="flow-playground-host-step-editor"
        data-testid="host-step-editor"
      >
        <h3>{zh ? '审批人要看的内容' : 'What the reviewer checks'}</h3>
        <p>
          {zh
            ? '勾选审批时要展示给审批人的上游输出。'
            : 'Pick the upstream outputs the reviewer is shown.'}
        </p>
        {outputs.map((output) => (
          <label
            key={output}
            className="flow-playground-host-step-editor__option"
          >
            <input
              checked={selected.has(output)}
              data-testid={`host-approval-input-${output}`}
              onChange={(event) =>
                onApprovalInputs(
                  node.id,
                  event.currentTarget.checked
                    ? [...selected, output]
                    : [...selected].filter((item) => item !== output),
                )
              }
              type="checkbox"
            />
            <code>{output}</code>
          </label>
        ))}
      </section>
    );
  }
  const chain = hostConditionChain(graph, node.id);
  const chainIds = new Set(chain.map((item) => item.id));
  const source = hostConditionInput(graph, node.id);
  const otherwise = new Set(hostOtherwiseTargets(graph, node.id));
  const steps = graph.nodes.filter(
    (item) =>
      item.data.hostPlanStep &&
      !chainIds.has(item.id) &&
      typeof item.data.hostConditionPart !== 'string',
  );
  return (
    <section
      className="flow-playground-host-step-editor"
      data-testid="host-step-editor"
    >
      <h3>{zh ? '条件分支' : 'Condition'}</h3>
      <label className="flow-playground-host-step-editor__field">
        <span>
          {zh
            ? '判断依据（读哪一步的哪个输出）'
            : 'Routes on (which step output)'}
        </span>
        <select
          data-testid="host-condition-input"
          onChange={(event) =>
            onConditionInput(chain[0]?.id ?? node.id, event.currentTarget.value)
          }
          value={source}
        >
          <option value="">{zh ? '（未选择）' : '(none)'}</option>
          {outputs.map((output) => (
            <option key={output} value={output}>
              {output}
            </option>
          ))}
        </select>
      </label>
      <p>
        {zh
          ? `共 ${chain.length} 个分支；分支标签在下方“True branch label”里改，命中后运行的步骤由“Matched”出口的连线决定。`
          : `${chain.length} case(s); edit a case label in "True branch label" below, and wire its "Matched" port to the steps it runs.`}
      </p>
      <fieldset className="flow-playground-host-step-editor__group">
        <legend>
          {zh
            ? '兜底分支（没有分支命中时运行）'
            : 'Otherwise (runs when no case matches)'}
        </legend>
        {steps.map((step) => (
          <label
            key={step.id}
            className="flow-playground-host-step-editor__option"
          >
            <input
              checked={otherwise.has(step.id)}
              data-testid={`host-otherwise-${step.id}`}
              onChange={(event) =>
                onOtherwise(
                  node.id,
                  event.currentTarget.checked
                    ? [...otherwise, step.id]
                    : [...otherwise].filter((item) => item !== step.id),
                )
              }
              type="checkbox"
            />
            <span>{stepTitle(step)}</span>
          </label>
        ))}
      </fieldset>
    </section>
  );
}
