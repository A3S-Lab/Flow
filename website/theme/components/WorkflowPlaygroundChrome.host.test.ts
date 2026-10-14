import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { workflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  HostPlanNotes,
  planNoteSeverityLabel,
  WorkflowPlaygroundHeader,
} from './WorkflowPlaygroundChrome';

const baseProps = {
  backHref: '/',
  backLabel: 'Back',
  copy: workflowPlaygroundCopy.zh,
  languageHref: '/playground',
  logoSrc: '/a3s-logo.png',
  onExport: () => {},
  onOpenDocument: () => {},
  onOpenExtensions: () => {},
  onReset: () => {},
  onRunToggle: () => {},
  onValidate: () => {},
  issueCount: 0,
  running: false,
  saveState: 'saved' as const,
  version: 'v1.0.0',
  versions: ['v1.0.0'],
  workflowName: 'Example workflow',
  onVersionChange: () => {},
  hostMode: true,
  locale: 'zh' as const,
};

const notes = [
  {
    severity: 'error' as const,
    code: 'BRANCH_DELIVERS_NOTHING',
    step_id: 'route',
    message: '走 otherwise 时没有步骤交付结果',
  },
  {
    severity: 'warning' as const,
    code: 'RATIONALE_MISSING',
    step_id: 'tax',
    message: '缺少选择理由',
  },
  {
    severity: 'warning' as const,
    code: 'REQUIRED_CAPABILITY_UNUSED',
    step_id: null,
    message: '任务要求的能力没有步骤使用',
  },
];

describe('WorkflowPlayground host header: design notes, edit origin, re-plan', () => {
  it('lists every design note with its kind, rule, step and message once opened', () => {
    const closed = renderToStaticMarkup(
      createElement(HostPlanNotes, { locale: 'zh', notes }),
    );
    expect(closed).toContain('设计提示 3');
    expect(closed).not.toContain('data-testid="host-plan-notes"');
    const open = renderToStaticMarkup(
      createElement(HostPlanNotes, {
        locale: 'zh',
        notes,
        defaultOpen: true,
        onSelectPlanStep: () => {},
      }),
    );
    expect(open).toContain('不会阻止保存或批准');
    expect(open.match(/data-testid="host-plan-note"/g)).toHaveLength(3);
    for (const note of notes) {
      expect(open).toContain(note.code);
      expect(open).toContain(note.message);
    }
    expect(open).toContain('结构问题');
    expect(open).toContain('建议');
    // A plan-level note has no step to jump to.
    const items = open.split('<li').slice(1);
    expect(items.map((item) => item.includes('disabled=""'))).toEqual([
      false,
      false,
      true,
    ]);
    expect(planNoteSeverityLabel(notes[0], 'en')).toBe('Structural');
  });

  it('says when a person edited the plan and offers re-planning only when asked to', () => {
    const edited = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        planOrigin: 'human_edit',
        planWarnings: notes,
        onHostReplan: () => {},
      }),
    );
    expect(edited).toContain('AI 编排 · 已人工修改');
    expect(edited).toContain('设计提示 3');
    expect(edited).toContain('data-testid="host-replan"');
    expect(edited).toContain('让 AI 重新规划');
    const drafted = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        planOrigin: 'orchestrator',
      }),
    );
    expect(drafted).toContain('由 AI 编排生成');
    expect(drafted).not.toContain('data-testid="host-replan"');
  });
});
