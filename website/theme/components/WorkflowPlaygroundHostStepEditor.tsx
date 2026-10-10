import type { FlowWebsiteLocale } from './flow-node-catalog';
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
};

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
}: WorkflowPlaygroundHostStepEditorProps) {
  const kind = hostStepEditorKind(node);
  if (!kind) return null;
  const zh = locale === 'zh';
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
