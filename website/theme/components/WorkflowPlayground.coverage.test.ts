import {
  createWorkflowNodeDefaultValue,
  createWorkflowNodeForm,
  WORKFLOW_CONFIGURATION_WIDGETS,
  WORKFLOW_NODE_FIELD_GROUPS,
  type WorkflowNodeFieldDefinition,
} from '@a3s-lab/flow-ui';
import { describe, expect, it } from 'vitest';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import { createSampleWorkflow } from './WorkflowPlayground.sample';

function serialized(value: unknown): string {
  return JSON.stringify(value);
}

function canShowAlternative(field: WorkflowNodeFieldDefinition): boolean {
  if (field.readonly || field.show === false) return false;
  if (field.type === 'data_display') return false;
  return true;
}

describe('Workflow Playground manifest coverage', () => {
  it('renders the complete configuration-control matrix inside the business example', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const controls = new Map<string, string[]>();

    for (const manifest of catalog.registry.list()) {
      const document = createWorkflowNodeForm(manifest, {
        locale: 'en',
        presentation: 'task',
      });
      const exampleNodes = sample.nodes.filter(
        ({ data }) => data.dagNode.data.type === manifest.type,
      );
      for (const field of manifest.fields.filter(
        ({ show }) => show !== false,
      )) {
        const formNode = document.ui.nodes.find(
          (node) => node.schemaPath === `/properties/${field.name}`,
        );
        const control = formNode?.customProps?.controlWidget;
        if (typeof control !== 'string') continue;
        const configured = exampleNodes.some(({ data }) =>
          Object.hasOwn(data.dagNode.data, field.name),
        );
        if (!configured) continue;
        controls.set(control, [
          ...(controls.get(control) ?? []),
          `${manifest.type}.${field.name}`,
        ]);
      }
    }

    const expected = [
      WORKFLOW_CONFIGURATION_WIDGETS.actionPicker,
      WORKFLOW_CONFIGURATION_WIDGETS.code,
      WORKFLOW_CONFIGURATION_WIDGETS.connection,
      WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay,
      WORKFLOW_CONFIGURATION_WIDGETS.duration,
      WORKFLOW_CONFIGURATION_WIDGETS.file,
      WORKFLOW_CONFIGURATION_WIDGETS.flowBatch,
      WORKFLOW_CONFIGURATION_WIDGETS.flowChildren,
      WORKFLOW_CONFIGURATION_WIDGETS.flowExpression,
      WORKFLOW_CONFIGURATION_WIDGETS.flowSchema,
      WORKFLOW_CONFIGURATION_WIDGETS.flowSpec,
      WORKFLOW_CONFIGURATION_WIDGETS.json,
      WORKFLOW_CONFIGURATION_WIDGETS.mcp,
      WORKFLOW_CONFIGURATION_WIDGETS.model,
      WORKFLOW_CONFIGURATION_WIDGETS.prompt,
      WORKFLOW_CONFIGURATION_WIDGETS.sortableList,
      WORKFLOW_CONFIGURATION_WIDGETS.tabs,
      'multi-select',
      'number',
      'password',
      'select',
      'slider',
      'switch',
      'tags',
      'text',
      'textarea',
    ].sort();

    expect([...controls.keys()].sort()).toEqual(expected);
    for (const control of expected) {
      expect(controls.get(control)?.length ?? 0, control).toBeGreaterThan(0);
    }
  });

  it('gives every manifest field an explicit, rendered Playground example', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const groups = new Set(Object.keys(WORKFLOW_NODE_FIELD_GROUPS));
    const issues: string[] = [];

    for (const manifest of catalog.registry.list()) {
      const examples = sample.nodes.filter(
        (node) => node.data.dagNode.data.type === manifest.type,
      );
      const defaults = createWorkflowNodeDefaultValue(manifest);
      const document = createWorkflowNodeForm(manifest, {
        locale: 'en',
        presentation: 'task',
      });

      if (examples.length === 0) {
        issues.push(`${manifest.type}: missing sample node`);
      }

      for (const field of manifest.fields) {
        if (field.show === false) continue;
        const formNode = document.ui.nodes.find(
          (node) => node.schemaPath === `/properties/${field.name}`,
        );
        const configuredValues = examples.flatMap((node) =>
          Object.hasOwn(node.data.dagNode.data, field.name)
            ? [node.data.dagNode.data[field.name]]
            : [],
        );

        const identity = `${manifest.type}.${field.name}`;
        if (!formNode) issues.push(`${identity}: missing form control`);
        if (typeof formNode?.customProps?.controlWidget !== 'string') {
          issues.push(`${identity}: missing control contract`);
        }
        if (!groups.has(String(formNode?.customProps?.semanticGroup))) {
          issues.push(`${identity}: missing semantic panel group`);
        }
        if (configuredValues.length === 0) {
          issues.push(`${identity}: only implied by its default`);
        }

        if (!canShowAlternative(field)) continue;
        if (
          !configuredValues.some(
            (value) => serialized(value) !== serialized(defaults[field.name]),
          )
        ) {
          issues.push(`${identity}: never demonstrates an edited value`);
        }
      }
    }

    expect(issues).toEqual([]);
  });

  it('demonstrates both states of every conditional Hook field', () => {
    const catalog = createPlaygroundNodeCatalog('en');
    const sample = createSampleWorkflow('en', catalog);
    const manifest = catalog.registry.require('flow.hook');
    const hooks = sample.nodes.filter(
      ({ data }) => data.dagNode.data.type === manifest.type,
    );

    for (const field of manifest.fields.filter(
      ({ visible_when }) => visible_when,
    )) {
      const condition = field.visible_when!;
      const visibility = hooks.map(
        ({ data }) => data.dagNode.data[condition.field] === condition.equals,
      );
      expect(visibility, field.name).toContain(true);
      expect(visibility, field.name).toContain(false);
    }
  });
});
