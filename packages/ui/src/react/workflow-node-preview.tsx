import type {
  WorkflowNodeDefinition,
  WorkflowNodeFieldDefinition,
} from "../integrations/workflow-node-manifest";
import { DesignerIcon } from "./designer-icons";
import {
  type WorkflowNodeFamily,
  workflowNodeVisual,
} from "./workflow-node-visual";

export type WorkflowNodePreviewStatus =
  "idle" | "running" | "success" | "waiting" | "error";

export interface WorkflowNodePreviewProps {
  node: WorkflowNodeDefinition;
  className?: string;
  selected?: boolean;
  ports?: WorkflowNodePreviewPorts;
  locale?: string;
  technical?: boolean;
  status?: WorkflowNodePreviewStatus;
  summary?: readonly WorkflowNodePreviewSummaryItem[];
  onSelect?: () => void;
  onRequestNext?: (port: WorkflowNodePreviewPort) => void;
}

export interface WorkflowNodePreviewSummaryItem {
  id: string;
  label: string;
  value: string;
}

export interface WorkflowNodePreviewPort {
  id: string;
  label: string;
  types: readonly string[];
  kind?: "control" | "data";
}

export interface WorkflowNodePreviewPorts {
  inputs: readonly WorkflowNodePreviewPort[];
  outputs: readonly WorkflowNodePreviewPort[];
}

function uniqueTypes(values: readonly string[]): string[] {
  return [...new Set(values.filter(Boolean))];
}

function previewShape(
  family: WorkflowNodeFamily,
  inputs: readonly WorkflowNodePreviewPort[],
  outputs: readonly WorkflowNodePreviewPort[],
): WorkflowNodeFamily | "isolated" {
  if (family === "container" || family === "entry" || family === "terminal")
    return family;
  if (inputs.length === 0 && outputs.length === 0) return "isolated";
  if (inputs.length === 0) return "entry";
  const controlOutputs = outputs.filter((port) => port.kind === "control");
  if (outputs.length === 0) return "terminal";
  if (controlOutputs.length > 1) return "branch";
  return family;
}

function isVisibleInputPortField(
  field: WorkflowNodeFieldDefinition,
): field is WorkflowNodeFieldDefinition & { input_types: string[] } {
  return field.show !== false && (field.input_types?.length ?? 0) > 0;
}

function inputPorts(node: WorkflowNodeDefinition): WorkflowNodePreviewPort[] {
  const fields = node.fields.filter(isVisibleInputPortField).map((field) => ({
    id: field.name,
    label: field.display_name ?? field.name,
    types: uniqueTypes(field.input_types),
    kind: "data" as const,
  }));
  if (fields.length > 0) return fields;
  const types = uniqueTypes(node.input_types);
  return types.length > 0
    ? [{ id: "input", label: "Input", types, kind: "data" }]
    : [];
}

function outputPorts(node: WorkflowNodeDefinition): WorkflowNodePreviewPort[] {
  if (node.outputs.length > 0) {
    return node.outputs.map((output) => ({
      id: output.name,
      label: output.display_name || output.name,
      types: uniqueTypes(output.types),
      kind: output.types.includes("FlowControl")
        ? ("control" as const)
        : ("data" as const),
    }));
  }
  const types = uniqueTypes(node.output_types);
  return types.length > 0
    ? [{ id: "output", label: "Output", types, kind: "data" }]
    : [];
}

function previewCopy(chinese: boolean) {
  return chinese
    ? {
        inputPorts: "输入端口",
        outputPorts: "输出端口",
        control: "控制流",
        data: "数据",
        any: "任意类型",
        addNext: (label: string) => `从「${label}」添加下一个节点`,
        containerScope: "容器内部画布",
        containerStart: "容器起点",
        containerHint: "后续节点在这个作用域内运行",
        configurationSummary: "配置摘要",
        entry: "开始",
        status: {
          running: "运行中",
          success: "运行成功",
          waiting: "等待中",
          error: "运行失败",
        },
      }
    : {
        inputPorts: "Input ports",
        outputPorts: "Output ports",
        control: "Control",
        data: "Data",
        any: "Any",
        addNext: (label: string) => `Add next node from ${label}`,
        containerScope: "Container canvas",
        containerStart: "Scope start",
        containerHint: "Following nodes run inside this scope",
        configurationSummary: "Current configuration",
        entry: "Start",
        status: {
          running: "Running",
          success: "Succeeded",
          waiting: "Waiting",
          error: "Failed",
        },
      };
}

function CompactPortList({
  direction,
  ports,
  shape,
  copy,
  onRequestNext,
}: {
  direction: "input" | "output";
  ports: readonly WorkflowNodePreviewPort[];
  shape: WorkflowNodeFamily | "isolated";
  copy: ReturnType<typeof previewCopy>;
  onRequestNext?: (port: WorkflowNodePreviewPort) => void;
}) {
  if (ports.length === 0) return null;
  return (
    <section
      className="a3s-form-workflow-node-preview-compact-ports"
      data-direction={direction}
      aria-label={direction === "input" ? copy.inputPorts : copy.outputPorts}
    >
      <ul>
        {ports.map((port) => {
          const kind = port.kind ?? "data";
          const canAddNext =
            direction === "output" && kind === "control" && onRequestNext;
          const showLabel = Boolean(canAddNext && shape === "branch");
          return (
            <li
              data-port-id={port.id}
              data-port-kind={kind}
              data-label-visible={showLabel || undefined}
              key={port.id}
            >
              {direction === "input" && <i aria-hidden="true" />}
              {showLabel && <strong>{port.label}</strong>}
              {canAddNext && (
                <button
                  type="button"
                  className="a3s-form-workflow-node-preview-next"
                  aria-label={copy.addNext(port.label)}
                  onClick={() => onRequestNext(port)}
                >
                  <span aria-hidden="true">+</span>
                </button>
              )}
              {direction === "output" && <i aria-hidden="true" />}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function PortList({
  direction,
  ports,
  technical,
  copy,
  onRequestNext,
}: {
  direction: "input" | "output";
  ports: readonly WorkflowNodePreviewPort[];
  technical: boolean;
  copy: ReturnType<typeof previewCopy>;
  onRequestNext?: (port: WorkflowNodePreviewPort) => void;
}) {
  if (ports.length === 0) return null;
  return (
    <section
      className="a3s-form-workflow-node-preview-port-list"
      data-direction={direction}
      aria-label={direction === "input" ? copy.inputPorts : copy.outputPorts}
    >
      <span
        className="a3s-form-workflow-node-preview-port-heading"
        aria-hidden="true"
      >
        {direction === "input" ? copy.inputPorts : copy.outputPorts}
      </span>
      <ul>
        {ports.map((port) => {
          const kind = port.kind ?? "data";
          const canAddNext =
            direction === "output" && kind === "control" && onRequestNext;
          return (
            <li
              className="a3s-form-workflow-node-preview-port"
              data-port-id={port.id}
              data-port-kind={kind}
              key={port.id}
            >
              {direction === "input" && <i aria-hidden="true" />}
              <span>
                <strong>{port.label}</strong>
                <small>
                  {technical
                    ? port.types.join(" · ") || copy.any
                    : kind === "control"
                      ? copy.control
                      : copy.data}
                </small>
              </span>
              {canAddNext && (
                <button
                  type="button"
                  className="a3s-form-workflow-node-preview-next"
                  aria-label={copy.addNext(port.label)}
                  onClick={() => onRequestNext(port)}
                >
                  <span aria-hidden="true">+</span>
                </button>
              )}
              {direction === "output" && <i aria-hidden="true" />}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

export function WorkflowNodePreview({
  node,
  className,
  ports,
  selected = true,
  locale = "en",
  technical = true,
  status = "idle",
  summary = [],
  onSelect,
  onRequestNext,
}: WorkflowNodePreviewProps) {
  const chinese = locale.toLocaleLowerCase().startsWith("zh");
  const copy = previewCopy(chinese);
  const inputs = ports?.inputs ?? inputPorts(node);
  const outputs = ports?.outputs ?? outputPorts(node);
  const runtimeBinding =
    "runtimeBinding" in node && typeof node.runtimeBinding === "string"
      ? node.runtimeBinding
      : undefined;
  const visual = workflowNodeVisual(node);
  const shape = previewShape(visual.family, inputs, outputs);
  const interactive = typeof onSelect === "function";
  const accessibleName = `${node.display_name}${chinese ? "节点预览" : " workflow node preview"}`;
  return (
    <article
      className={["a3s-form-workflow-node-preview", className]
        .filter(Boolean)
        .join(" ")}
      data-node-type={node.type}
      data-node-family={visual.family}
      data-node-shape={shape}
      data-node-tone={visual.tone}
      data-runtime-binding={runtimeBinding}
      data-selected={selected || undefined}
      data-status={status}
      data-technical={technical || undefined}
      data-has-summary={summary.length > 0 || undefined}
      aria-label={interactive ? undefined : accessibleName}
    >
      {interactive && (
        <button
          type="button"
          className="a3s-form-workflow-node-preview-select"
          data-node-family={visual.family}
          data-node-shape={shape}
          data-node-tone={visual.tone}
          data-node-type={node.type}
          data-status={status}
          aria-label={accessibleName}
          aria-pressed={selected}
          onClick={onSelect}
        />
      )}
      {!technical && shape === "entry" && (
        <span className="a3s-form-workflow-node-preview-entry-label">
          {copy.entry}
        </span>
      )}
      <header>
        <span aria-hidden="true">
          <DesignerIcon name={visual.icon} size={17} />
        </span>
        <div>
          <strong>{node.display_name}</strong>
          {technical && <small>{node.categoryLabel}</small>}
        </div>
        {status !== "idle" && (
          <span
            className="a3s-form-workflow-node-preview-status"
            data-status={status}
            aria-label={copy.status[status]}
            role="status"
          >
            <i aria-hidden="true" />
            <span>{copy.status[status]}</span>
          </span>
        )}
        {(node.beta || node.legacy) && (
          <em>
            {node.beta
              ? chinese
                ? "测试版"
                : "Beta"
              : chinese
                ? "旧版"
                : "Legacy"}
          </em>
        )}
      </header>
      <div className="a3s-form-workflow-node-preview-body">
        {summary.length > 0 && (
          <dl
            className="a3s-form-workflow-node-preview-summary"
            aria-label={copy.configurationSummary}
          >
            {summary.map((entry) => (
              <div data-summary-id={entry.id} key={entry.id}>
                <dt>{entry.label}</dt>
                <dd title={entry.value}>{entry.value}</dd>
              </div>
            ))}
          </dl>
        )}
        {technical ? (
          <PortList
            direction="input"
            ports={inputs}
            technical={technical}
            copy={copy}
          />
        ) : (
          <CompactPortList
            direction="input"
            ports={inputs}
            shape={shape}
            copy={copy}
          />
        )}
        {shape === "container" && (
          <section
            className="a3s-form-workflow-node-preview-container"
            aria-label={copy.containerScope}
          >
            <span className="a3s-form-workflow-node-preview-container-start">
              <DesignerIcon name="play" size={13} />
              <strong>{copy.containerStart}</strong>
            </span>
            {technical && <small>{copy.containerHint}</small>}
          </section>
        )}
        {technical ? (
          <PortList
            direction="output"
            ports={outputs}
            technical={technical}
            copy={copy}
            onRequestNext={onRequestNext}
          />
        ) : (
          <CompactPortList
            direction="output"
            ports={outputs}
            shape={shape}
            copy={copy}
            onRequestNext={onRequestNext}
          />
        )}
        {technical && inputs.length === 0 && outputs.length === 0 && (
          <p>
            {chinese
              ? "这个节点不需要连接端口。"
              : "This node does not use connection ports."}
          </p>
        )}
      </div>
      {node.description && (
        <p className="a3s-form-workflow-node-preview-description">
          {node.description}
        </p>
      )}
      <footer>
        {technical ? (
          <>
            <code>{node.type}</code>
            <span>
              {runtimeBinding ??
                (chinese
                  ? `${node.fields.filter((field) => field.show !== false).length} 项配置`
                  : `${node.fields.filter((field) => field.show !== false).length} settings`)}
            </span>
          </>
        ) : (
          <span>
            {chinese
              ? `${inputs.length} 个输入 · ${outputs.length} 个输出`
              : `${inputs.length} inputs · ${outputs.length} outputs`}
          </span>
        )}
      </footer>
    </article>
  );
}
