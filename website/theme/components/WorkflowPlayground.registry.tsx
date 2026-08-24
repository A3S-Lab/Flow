import {
  a3sFlowDagNodeRegistry,
  type A3SFlowDagNodeRegistry,
} from '@a3s-lab/flow-ui';
import { createContext, useContext } from 'react';

export const WorkflowPlaygroundRegistryContext =
  createContext<A3SFlowDagNodeRegistry>(a3sFlowDagNodeRegistry);

export function useWorkflowPlaygroundRegistry(): A3SFlowDagNodeRegistry {
  return useContext(WorkflowPlaygroundRegistryContext);
}
