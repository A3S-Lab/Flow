import type { WorkflowNodeDefinition } from '../integrations/workflow-node-manifest';
import type { DesignerIconName } from './designer-icons';

export type WorkflowNodeTone = 'blue' | 'cyan' | 'green' | 'orange' | 'red' | 'violet';
export type WorkflowNodeFamily =
  | 'entry'
  | 'action'
  | 'branch'
  | 'suspension'
  | 'terminal'
  | 'container';

interface WorkflowNodeVisualIdentity {
  icon: DesignerIconName;
  tone: WorkflowNodeTone;
  family: WorkflowNodeFamily;
}

const FLOW_NODE_VISUALS: Readonly<Record<string, WorkflowNodeVisualIdentity>> = {
  'flow.start': { icon: 'play', tone: 'blue', family: 'entry' },
  'flow.condition': { icon: 'layout', tone: 'cyan', family: 'branch' },
  'flow.complete': { icon: 'check-square', tone: 'green', family: 'terminal' },
  'flow.fail': { icon: 'alert', tone: 'red', family: 'terminal' },
  'flow.step': { icon: 'settings', tone: 'blue', family: 'action' },
  'flow.batch': { icon: 'list', tone: 'blue', family: 'action' },
  'flow.wait': { icon: 'calendar', tone: 'violet', family: 'suspension' },
  'flow.hook': { icon: 'link', tone: 'violet', family: 'suspension' },
  'flow.cancel': { icon: 'close', tone: 'red', family: 'terminal' },
  'flow.timeout': { icon: 'calendar', tone: 'red', family: 'terminal' },
  'flow.continue-as-new': { icon: 'redo', tone: 'cyan', family: 'action' },
  'flow.progress': { icon: 'slider', tone: 'cyan', family: 'action' },
  'flow.child-operation': { icon: 'link', tone: 'blue', family: 'action' },
  'flow.child-workflow': { icon: 'components', tone: 'blue', family: 'action' },
  'flow.child-workflows': { icon: 'list', tone: 'blue', family: 'action' },
  'flow.signal': { icon: 'radio', tone: 'violet', family: 'suspension' },
  iteration: { icon: 'list', tone: 'green', family: 'container' },
  'iteration-start': { icon: 'play', tone: 'green', family: 'entry' },
  loop: { icon: 'redo', tone: 'green', family: 'container' },
  'loop-start': { icon: 'play', tone: 'green', family: 'entry' },
};

function categoryIcon(category: string): DesignerIconName {
  const normalized = category.toLocaleLowerCase('en');
  if (normalized.includes('agent') || normalized.includes('model') || normalized.includes('llm')) {
    return 'sparkles';
  }
  if (normalized.includes('file') || normalized.includes('knowledge')) return 'file';
  if (normalized.includes('data') || normalized.includes('cassandra')) return 'grid';
  if (normalized.includes('flow') || normalized.includes('input')) return 'layout';
  if (normalized.includes('tool') || normalized.includes('utilit')) return 'calculator';
  if (normalized.includes('embedding') || normalized.includes('search')) return 'search';
  if (normalized.includes('processing')) return 'settings';
  return 'components';
}

/** Shared visual identity for canvas nodes and their configuration panels. */
export function workflowNodeVisual(node: WorkflowNodeDefinition): {
  icon: DesignerIconName;
  tone: WorkflowNodeTone;
  family: WorkflowNodeFamily;
} {
  const type = node.type.toLocaleLowerCase('en');
  const exact = FLOW_NODE_VISUALS[type];
  if (exact) return exact;
  const role = 'role' in node && typeof node.role === 'string' ? node.role : undefined;
  if (role === 'container') return { icon: 'layout', tone: 'green', family: 'container' };
  if (role === 'entry' || role === 'container-start') {
    return { icon: 'play', tone: 'blue', family: 'entry' };
  }
  if (/(condition|branch|classifier|switch)/u.test(type)) {
    return { icon: 'layout', tone: 'cyan', family: 'branch' };
  }
  if (/(complete|success|end)$/u.test(type)) {
    return { icon: 'check-square', tone: 'green', family: 'terminal' };
  }
  if (/(fail|error|reject|cancel|timeout)/u.test(type)) {
    return { icon: 'alert', tone: 'red', family: 'terminal' };
  }
  if (/(wait|schedule|delay|timer|signal)/u.test(type)) {
    return { icon: 'calendar', tone: 'violet', family: 'suspension' };
  }
  if (/(hook|callback|webhook|event)/u.test(type)) {
    return { icon: 'link', tone: 'violet', family: 'suspension' };
  }
  if (/(batch|iteration|loop|parallel)/u.test(type)) {
    return { icon: 'list', tone: 'green', family: 'container' };
  }
  if (/(start|trigger)/u.test(type)) return { icon: 'play', tone: 'blue', family: 'entry' };
  return { icon: categoryIcon(node.category), tone: 'blue', family: 'action' };
}
