import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import { createWorkflowExamples } from './WorkflowPlayground.examples';
import { WorkflowPlaygroundExamples } from './WorkflowPlaygroundExamples';
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
});
