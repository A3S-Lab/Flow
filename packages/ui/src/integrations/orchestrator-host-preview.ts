/**
 * Project registry for Orchestrator host preview nodes.
 *
 * Host-compiled `flow_dsl` uses public host types that are not in Flow's
 * built-in catalog. Playground host mode passes this registry explicitly so
 * the preview graph can render; edit authority remains `host.agent-plan.v2`.
 *
 * Publication-legal type is `orchestrator.agent.step` (≥3 namespace segments).
 * The current host still emits legacy `orchestrator.agent_step`; preview
 * loading aliases that string onto the registered manifest without treating
 * the alias as a second executable authority.
 */

import {
  createA3SFlowDagNodeCatalog,
  defineA3SFlowCustomDagNode,
  type A3SFlowCustomDagNodeRegistration,
  type A3SFlowDagNodeCatalog,
} from './a3s-flow-custom-node';
import {
  a3sFlowDagNodeRegistry,
  type A3SFlowDagNodeRegistry,
} from './a3s-flow-node-manifest';

/** Publication-legal public type for one planned agent step. */
export const ORCHESTRATOR_AGENT_STEP_TYPE = 'orchestrator.agent.step';

/**
 * Legacy discriminator still emitted by Orchestrator `compile_workflow_dsl`.
 * Preview loading maps it onto {@link ORCHESTRATOR_AGENT_STEP_TYPE}.
 */
export const ORCHESTRATOR_AGENT_STEP_TYPE_LEGACY = 'orchestrator.agent_step';

export const ORCHESTRATOR_HOST_PREVIEW_TYPE_ALIASES: Readonly<
  Record<string, string>
> = Object.freeze({
  [ORCHESTRATOR_AGENT_STEP_TYPE_LEGACY]: ORCHESTRATOR_AGENT_STEP_TYPE,
});

export function resolveOrchestratorHostPreviewType(type: string): string {
  return ORCHESTRATOR_HOST_PREVIEW_TYPE_ALIASES[type] ?? type;
}

export type OrchestratorHostPreviewLocale = 'zh' | 'en';

export function defineOrchestratorAgentStepRegistration(
  locale: OrchestratorHostPreviewLocale = 'en',
): A3SFlowCustomDagNodeRegistration {
  const zh = locale === 'zh';
  return defineA3SFlowCustomDagNode({
    manifest: {
      type: ORCHESTRATOR_AGENT_STEP_TYPE,
      display_name: zh ? '编排智能体步骤' : 'Orchestrator agent step',
      description: zh
        ? '宿主 AgentPlan 中的一步；仅作预览，编辑权威仍是 plan。'
        : 'One AgentPlan step from the host; preview only — plan stays authoritative.',
      category: 'orchestrator',
      categoryLabel: zh ? '编排器' : 'Orchestrator',
      role: 'host',
      icon: 'cpu-chip',
      ports: {
        inputs: [
          {
            id: 'in',
            label: zh ? '进入' : 'In',
            kind: 'control',
            types: ['FlowControl'],
          },
          {
            id: 'input',
            label: zh ? '输入' : 'Input',
            kind: 'data',
            types: ['JsonValue'],
          },
        ],
        outputs: [
          {
            id: 'success',
            label: zh ? '成功' : 'Success',
            kind: 'control',
            types: ['FlowControl'],
          },
          {
            id: 'result',
            label: zh ? '结果' : 'Result',
            kind: 'data',
            types: ['JsonValue'],
          },
        ],
      },
      input_types: ['FlowValue'],
      output_types: ['StepResult'],
      fields: [
        {
          name: 'agent_id',
          display_name: zh ? '智能体' : 'Agent',
          info: zh ? '计划中选定的 agent_id。' : 'agent_id selected in the plan.',
          type: 'str',
          _input_type: 'StrInput',
          value: '',
          required: true,
        },
        {
          name: 'objective',
          display_name: zh ? '目标' : 'Objective',
          info: zh ? '该步的 objective。' : 'Step objective.',
          type: 'str',
          _input_type: 'StrInput',
          value: '',
          required: true,
        },
        {
          name: 'version',
          display_name: zh ? '版本' : 'Version',
          info: zh ? 'Agent 版本。' : 'Agent version.',
          type: 'str',
          _input_type: 'StrInput',
          value: '1.0.0',
          required: false,
          advanced: true,
        },
      ],
      outputs: [
        {
          name: 'result',
          display_name: zh ? '结果' : 'Result',
          types: ['Json'],
          group_outputs: false,
          allows_loop: false,
          tool_mode: false,
        },
      ],
    },
    capability: {
      id: 'orchestrator/agent-step',
      version: '1.0.0',
      handler: 'orchestrator.agent_step',
    },
  });
}

export function createOrchestratorHostPreviewRegistrations(
  locale: OrchestratorHostPreviewLocale = 'en',
): A3SFlowCustomDagNodeRegistration[] {
  return [defineOrchestratorAgentStepRegistration(locale)];
}

/** Catalog = built-ins + Orchestrator preview registrations (explicit registry). */
export function createOrchestratorHostPreviewCatalog(
  locale: OrchestratorHostPreviewLocale = 'en',
  baseRegistry: A3SFlowDagNodeRegistry = a3sFlowDagNodeRegistry,
): A3SFlowDagNodeCatalog {
  return createA3SFlowDagNodeCatalog(
    createOrchestratorHostPreviewRegistrations(locale),
    baseRegistry,
  );
}

/**
 * Merge Orchestrator preview nodes into an existing project catalog
 * (e.g. Playground demo customs + orchestrator.agent.step).
 */
export function extendCatalogWithOrchestratorHostPreview(
  base: A3SFlowDagNodeCatalog,
  locale: OrchestratorHostPreviewLocale = 'en',
): A3SFlowDagNodeCatalog {
  return createA3SFlowDagNodeCatalog(
    [
      ...base.custom,
      ...createOrchestratorHostPreviewRegistrations(locale),
    ],
    // Rebuild from built-ins only; `base.custom` is re-applied above.
    a3sFlowDagNodeRegistry,
  );
}
