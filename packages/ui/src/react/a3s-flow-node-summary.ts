import type { JsonObject, JsonValue } from '@a3s-lab/ui/form/core';
import type { A3SFlowWorkflowDagNode } from '../integrations/a3s-flow-dsl-types';
import {
  type A3SFlowExpressionPurpose,
  a3sFlowExpressionFrom,
  a3sFlowExpressionPreviewText,
} from './a3s-flow-expression-format';
import type { WorkflowNodePreviewSummaryItem } from './workflow-node-preview';

function isObject(value: JsonValue | undefined): value is JsonObject {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function stringValue(value: JsonValue | undefined): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined;
}

function expressionValue(
  value: JsonValue | undefined,
  purpose: A3SFlowExpressionPurpose,
  chinese: boolean,
): string | undefined {
  const expression = a3sFlowExpressionFrom(value);
  return expression
    ? a3sFlowExpressionPreviewText(expression, purpose, chinese, 'compact')
    : undefined;
}

function workflowReference(value: JsonValue | undefined): string | undefined {
  if (!isObject(value)) return undefined;
  const name = stringValue(value.name);
  const version = stringValue(value.version);
  if (!name) return undefined;
  return version ? `${name} · ${version}` : name;
}

function firstObject(value: JsonValue | undefined): JsonObject | undefined {
  const first = Array.isArray(value) ? value[0] : undefined;
  return isObject(first) ? first : undefined;
}

function summaryCopy(chinese: boolean) {
  return chinese
    ? {
        workflow: '工作流',
        version: '版本',
        rule: '判断规则',
        output: '输出',
        message: '失败信息',
        handler: '任务',
        input: '输入',
        steps: '任务数',
        first: '首项任务',
        resume: '恢复时间',
        callback: '等待方式',
        title: '请求标题',
        state: '状态',
        ready: '无需设置',
        deadline: '截止时间',
        progress: '进度 ID',
        completed: '已完成',
        reference: '关联记录',
        operation: '任务类型',
        child: '子工作流 ID',
        children: '子工作流数',
        signal: '信号名称',
        wait: '等待 ID',
        items: '待遍历集合',
        limit: '最多循环',
        notSet: '未设置',
        humanApproval: '人工审批',
        webhook: 'Webhook',
        hostEvent: '应用事件',
      }
    : {
        workflow: 'Workflow',
        version: 'Version',
        rule: 'Rule',
        output: 'Output',
        message: 'Failure',
        handler: 'Handler',
        input: 'Input',
        steps: 'Steps',
        first: 'First step',
        resume: 'Resume at',
        callback: 'Callback',
        title: 'Title',
        state: 'State',
        ready: 'Ready to run',
        deadline: 'Deadline',
        progress: 'Progress ID',
        completed: 'Completed',
        reference: 'Reference ID',
        operation: 'Operation',
        child: 'Child ID',
        children: 'Children',
        signal: 'Signal',
        wait: 'Wait ID',
        items: 'Items',
        limit: 'Limit',
        notSet: 'Not set',
        humanApproval: 'Human approval',
        webhook: 'Webhook',
        hostEvent: 'Host event',
      };
}

function callbackKind(value: JsonValue | undefined, chinese: boolean): string | undefined {
  const copy = summaryCopy(chinese);
  switch (value) {
    case 'human_approval':
      return copy.humanApproval;
    case 'webhook':
      return copy.webhook;
    case 'host_event':
      return copy.hostEvent;
    default:
      return stringValue(value);
  }
}

function item(
  id: string,
  label: string,
  value: string | undefined,
): WorkflowNodePreviewSummaryItem {
  return { id, label, value: value ?? '' };
}

export function a3sFlowDagNodePreviewSummary(
  dagNode: A3SFlowWorkflowDagNode,
  locale: string | undefined,
): WorkflowNodePreviewSummaryItem[] {
  const chinese = locale?.toLocaleLowerCase().startsWith('zh') === true;
  const copy = summaryCopy(chinese);
  const data = dagNode.data;

  switch (data.type) {
    case 'flow.start':
      return [
        item('workflow', copy.workflow, stringValue(data.workflow_name) ?? copy.notSet),
        item('version', copy.version, stringValue(data.workflow_version) ?? copy.notSet),
      ];
    case 'flow.condition':
      return [
        item(
          'rule',
          copy.rule,
          expressionValue(data.expression, 'condition', chinese) ?? copy.notSet,
        ),
      ];
    case 'flow.complete':
      return [
        item(
          'output',
          copy.output,
          expressionValue(data.output_expression, 'output', chinese) ?? copy.notSet,
        ),
      ];
    case 'flow.fail':
      return [
        item(
          'message',
          copy.message,
          expressionValue(data.error_expression, 'error', chinese) ?? copy.notSet,
        ),
      ];
    case 'flow.step':
      return [
        item('handler', copy.handler, stringValue(data.step_name) ?? copy.notSet),
        item('input', copy.input, expressionValue(data.input, 'input', chinese) ?? copy.notSet),
      ];
    case 'flow.batch': {
      const steps = Array.isArray(data.steps) ? data.steps : [];
      const first = firstObject(data.steps);
      return [
        item('steps', copy.steps, String(steps.length)),
        item('first', copy.first, stringValue(first?.step_name) ?? copy.notSet),
      ];
    }
    case 'flow.wait':
      return [
        item(
          'resume',
          copy.resume,
          expressionValue(data.resume_at, 'datetime', chinese) ?? copy.notSet,
        ),
      ];
    case 'flow.hook':
      return [
        item('callback', copy.callback, callbackKind(data.kind, chinese) ?? copy.notSet),
        item('title', copy.title, stringValue(data.subject) ?? copy.notSet),
      ];
    case 'flow.cancel':
      return [item('state', copy.state, copy.ready)];
    case 'flow.timeout':
      return [
        item(
          'deadline',
          copy.deadline,
          expressionValue(data.deadline, 'datetime', chinese) ?? copy.notSet,
        ),
      ];
    case 'flow.continue-as-new':
      return [
        item('input', copy.input, expressionValue(data.input, 'input', chinese) ?? copy.notSet),
      ];
    case 'flow.progress':
      return [
        item('progress', copy.progress, stringValue(data.progress_id) ?? copy.notSet),
        item(
          'completed',
          copy.completed,
          expressionValue(data.completed, 'input', chinese) ?? copy.notSet,
        ),
      ];
    case 'flow.child-operation':
      return [
        item('reference', copy.reference, stringValue(data.reference_id) ?? copy.notSet),
        item('operation', copy.operation, stringValue(data.kind) ?? copy.notSet),
      ];
    case 'flow.child-workflow':
      return [
        item('child', copy.child, stringValue(data.child_id) ?? copy.notSet),
        item('workflow', copy.workflow, workflowReference(data.spec) ?? copy.notSet),
      ];
    case 'flow.child-workflows': {
      const children = Array.isArray(data.children) ? data.children : [];
      const first = firstObject(data.children);
      return [
        item('children', copy.children, String(children.length)),
        item('workflow', copy.workflow, workflowReference(first?.spec) ?? copy.notSet),
      ];
    }
    case 'flow.signal':
      return [
        item('signal', copy.signal, stringValue(data.signal_name) ?? copy.notSet),
        item('wait', copy.wait, stringValue(data.wait_id) ?? copy.notSet),
      ];
    case 'iteration':
      return [
        item('items', copy.items, expressionValue(data.items, 'input', chinese) ?? copy.notSet),
      ];
    case 'loop':
      return [
        item(
          'rule',
          copy.rule,
          expressionValue(data.condition, 'condition', chinese) ?? copy.notSet,
        ),
        item(
          'limit',
          copy.limit,
          typeof data.max_iterations === 'number' ? String(data.max_iterations) : copy.notSet,
        ),
      ];
    default:
      return [];
  }
}
