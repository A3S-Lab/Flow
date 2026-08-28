import {
  WORKFLOW_CONFIGURATION_WIDGETS,
  WORKFLOW_SELECT_WIDGET_ALIASES,
} from '../integrations/workflow-node-form';
import { A3SFlowBatchWidget } from './a3s-flow-batch-widget';
import {
  A3SFlowChildrenWidget,
  A3SFlowWorkflowSpecWidget,
} from './a3s-flow-child-widgets';
import { A3SFlowExpressionWidget } from './a3s-flow-expression-widget';
import { A3SFlowSchemaWidget } from './a3s-flow-schema-widget';
import { WorkflowSelectWidget } from './workflow-configuration-widgets';
import type { FormWidgetRegistry } from '@a3s-lab/ui/form/react';

export const a3sFlowWidgetRegistry: FormWidgetRegistry = {
  // Keep the standalone Flow registry safe for hosts that render a FormRenderer
  // directly instead of going through WorkflowNodeConfigurationPanel.
  ...Object.fromEntries(
    WORKFLOW_SELECT_WIDGET_ALIASES.map((alias) => [
      alias,
      WorkflowSelectWidget,
    ]),
  ),
  [WORKFLOW_CONFIGURATION_WIDGETS.flowBatch]: A3SFlowBatchWidget,
  [WORKFLOW_CONFIGURATION_WIDGETS.flowChildren]: A3SFlowChildrenWidget,
  [WORKFLOW_CONFIGURATION_WIDGETS.flowExpression]: A3SFlowExpressionWidget,
  [WORKFLOW_CONFIGURATION_WIDGETS.flowSchema]: A3SFlowSchemaWidget,
  [WORKFLOW_CONFIGURATION_WIDGETS.flowSpec]: A3SFlowWorkflowSpecWidget,
};
