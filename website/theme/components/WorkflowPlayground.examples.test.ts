import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import { createWorkflowExamples } from './WorkflowPlayground.examples';
import { WorkflowPlaygroundExamples } from './WorkflowPlaygroundExamples';
import { WorkflowPlaygroundHeader } from './WorkflowPlaygroundChrome';
import { WorkflowPlaygroundTriggerDialog } from './WorkflowPlaygroundTriggerDialog';
import { workflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  compilePlaygroundGraph,
  validatePlaygroundConfigurations,
} from './WorkflowPlayground.model';

describe('Workflow Playground example library', () => {
  it.each(['zh', 'en'] as const)(
    'publishes every %s example as a valid editable workflow',
    (locale) => {
      const catalog = createPlaygroundNodeCatalog(locale);
      const examples = createWorkflowExamples(locale, catalog);

      expect(examples.length).toBeGreaterThanOrEqual(5);
      expect(new Set(examples.map(({ id }) => id)).size).toBe(examples.length);
      expect(examples.filter(({ featured }) => featured)).toHaveLength(1);

      for (const example of examples) {
        expect(example.graph.nodes.length, example.id).toBeGreaterThan(0);
        expect(
          compilePlaygroundGraph(
            example.graph.nodes,
            example.graph.edges,
            catalog,
          ),
          example.id,
        ).toMatchObject({ ok: true });
        expect(
          validatePlaygroundConfigurations(
            example.graph.nodes,
            example.graph.edges,
            catalog.registry,
          ),
          example.id,
        ).toEqual([]);
      }
    },
  );

  it('keeps the complete node showcase as the featured example', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const examples = createWorkflowExamples('en', catalog);
    const featured = examples.find(({ featured }) => featured);
    const registeredTypes = catalog.registry
      .list()
      .map(({ type }) => type)
      .sort();
    const featuredTypes = [
      ...new Set(
        featured?.graph.nodes.map(({ data }) => data.dagNode.data.type),
      ),
    ].sort();

    expect(featured?.id).toBe('cross-border-fulfillment');
    expect(featuredTypes).toEqual(registeredTypes);
  });

  it('renders the non-featured workflows as semantic grid cards', () => {
    const catalog = createPlaygroundNodeCatalog('zh');
    const examples = createWorkflowExamples('zh', catalog);
    const html = renderToStaticMarkup(
      createElement(WorkflowPlaygroundExamples, {
        exampleHref: (exampleId) => `/playground?example=${exampleId}`,
        examples,
        homeHref: '/',
        languageHref: '/en/playground',
        locale: 'zh',
        logoSrc: '/a3s-logo.png',
        onSelect: () => {},
        onVersionChange: () => {},
        version: 'v1.0.0',
        versionHref: (version) => `/${version}`,
        versions: ['v1.0.0'],
      }),
    );
    const grid = html.match(
      /<ul class="a3s-example-grid">([\s\S]*?)<\/ul>/u,
    )?.[1];

    expect(grid).toBeDefined();
    expect(grid?.match(/<li>/gu)).toHaveLength(examples.length - 1);
    expect(grid?.match(/class="a3s-example-card"/gu)).toHaveLength(
      examples.length - 1,
    );
    expect(grid).not.toContain('role="listitem"');
    for (const example of examples.filter(({ featured }) => !featured)) {
      expect(grid).toContain(`/playground?example=${example.id}`);
    }
  });

  it('renders website selectors through the Flow A3S UI contract', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const examples = createWorkflowExamples('en', catalog);
    const examplesHtml = renderToStaticMarkup(
      createElement(WorkflowPlaygroundExamples, {
        exampleHref: (exampleId) => `/playground?example=${exampleId}`,
        examples,
        homeHref: '/',
        languageHref: '/zh/playground',
        locale: 'en',
        logoSrc: '/a3s-logo.png',
        onSelect: () => {},
        onVersionChange: () => {},
        version: 'v1.0.0',
        versionHref: (version) => `/${version}`,
        versions: ['v1.0.0', 'v1.1.0'],
      }),
    );
    expect(examplesHtml).not.toContain('<select');
    expect(examplesHtml).toContain('data-a3s-select="flow"');
    expect(examplesHtml).toContain('role="combobox"');

    const headerHtml = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        backHref: '/',
        backLabel: 'Back',
        copy: workflowPlaygroundCopy.en,
        languageHref: '/zh/playground',
        locale: 'en',
        logoSrc: '/a3s-logo.png',
        onExport: () => {},
        onOpenDocument: () => {},
        onOpenExtensions: () => {},
        onReset: () => {},
        onRunToggle: () => {},
        onValidate: () => {},
        issueCount: 0,
        running: false,
        saveState: 'saved',
        version: 'v1.0.0',
        versions: ['v1.0.0', 'v1.1.0'],
        workflowName: 'Example workflow',
        onVersionChange: () => {},
      }),
    );
    expect(headerHtml).not.toContain('<select');
    expect(headerHtml).toContain('id="workflow-playground-version"');
    expect(headerHtml).toContain('role="combobox"');
  });

  it('renders the host-mode progress pill only while a compose-plan draft is in flight', () => {
    const baseProps = {
      backHref: '/',
      backLabel: 'Back',
      copy: workflowPlaygroundCopy.en,
      languageHref: '/zh/playground',
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
    };

    const withDigest = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        locale: 'zh',
        proposalDigest:
          'sha256:504a8a69c20dd1e9cc872166bacda5b4f87db17c69be4d83d92c69d22b26f50d',
      }),
    );
    expect(withDigest).toContain('由 AI 编排生成');
    expect(withDigest).toContain('方案 #504a8a');
    expect(withDigest).not.toContain('宿主');
    expect(withDigest).not.toContain('data-testid="host-proposal-digest"');

    const withoutProgress = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, { ...baseProps, locale: 'en' }),
    );
    expect(withoutProgress).not.toContain('data-testid="host-progress"');

    const withProgressEn = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        locale: 'en',
        progress: { stepIndex: 1, total: 2, agentId: 'frontend-developer' },
      }),
    );
    expect(withProgressEn).toContain('data-testid="host-progress"');
    expect(withProgressEn).toContain('Drafting step 1/2 · frontend-developer');

    const withProgressZh = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        locale: 'zh',
        progress: { stepIndex: 1, total: 2, agentId: 'frontend-developer' },
      }),
    );
    expect(withProgressZh).toContain('正在起草 step 1/2 · frontend-developer');

    const withUnknownTotal = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        locale: 'en',
        progress: { stepIndex: 1, total: null, agentId: 'frontend-developer' },
      }),
    );
    expect(withUnknownTotal).toContain(
      'Drafting step 1/? · frontend-developer',
    );

    const withUnknownTotalZh = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        locale: 'zh',
        progress: { stepIndex: 1, total: null, agentId: 'frontend-developer' },
      }),
    );
    expect(withUnknownTotalZh).toContain(
      '正在起草 step 1/? · frontend-developer',
    );

    const withoutHostMode = renderToStaticMarkup(
      createElement(WorkflowPlaygroundHeader, {
        ...baseProps,
        hostMode: false,
        locale: 'en',
        progress: { stepIndex: 1, total: 2, agentId: 'frontend-developer' },
      }),
    );
    expect(withoutHostMode).not.toContain('data-testid="host-progress"');
  });

  it('keeps trigger enum values lossless while rendering readable object labels', () => {
    const schema = {
      type: 'object' as const,
      properties: {
        mode: { type: 'string' as const, enum: ['safe', 'fast'] },
        policy: {
          type: 'object' as const,
          enum: [{ b: 1, a: 'strict' }],
        },
        enabled: { type: 'boolean' as const, enum: [true, false] },
        nullable: {
          type: ['string', 'null'] as ('string' | 'null')[],
          enum: [null, ''],
        },
      },
    };
    const html = renderToStaticMarkup(
      createElement(WorkflowPlaygroundTriggerDialog, {
        copy: workflowPlaygroundCopy.en,
        locale: 'en',
        onClose: () => {},
        onSubmit: () => {},
        schema,
        workflowName: 'Example workflow',
      }),
    );
    expect(html).not.toContain('<select');
    expect(html).toContain('role="combobox"');
    expect(html).toContain('a3s-json:');
    expect(html).not.toContain('role="option">a3s-json:');
    expect(html).toContain(
      'role="option">{&quot;a&quot;:&quot;strict&quot;,&quot;b&quot;:1}',
    );
  });
});
