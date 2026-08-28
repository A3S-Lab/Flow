import { useEffect } from 'react';

type WorkflowPlaygroundKeyboardOptions = {
  beginEdgeLabelEdit?: (edgeId: string) => void;
  deleteSelection: () => void;
  dismissPanels: () => void;
  duplicateNode: (nodeId: string) => void;
  redo: () => void;
  selectedEdgeId?: string;
  selectedNodeId?: string;
  undo: () => void;
};

export function useWorkflowPlaygroundKeyboard({
  beginEdgeLabelEdit,
  deleteSelection,
  dismissPanels,
  duplicateNode,
  redo,
  selectedEdgeId,
  selectedNodeId,
  undo,
}: WorkflowPlaygroundKeyboardOptions) {
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        target instanceof HTMLSelectElement ||
        (target instanceof HTMLElement &&
          target.closest('[role="combobox"]') !== null) ||
        (target instanceof HTMLElement && target.isContentEditable)
      ) {
        return;
      }
      const command = event.ctrlKey || event.metaKey;
      if (command && event.key.toLocaleLowerCase() === 'z') {
        event.preventDefault();
        event.shiftKey ? redo() : undo();
      } else if (
        command &&
        event.key.toLocaleLowerCase() === 'd' &&
        selectedNodeId
      ) {
        event.preventDefault();
        duplicateNode(selectedNodeId);
      } else if (
        (event.key === 'Enter' || event.key === 'F2') &&
        selectedEdgeId &&
        beginEdgeLabelEdit
      ) {
        event.preventDefault();
        beginEdgeLabelEdit(selectedEdgeId);
      } else if (event.key === 'Delete' || event.key === 'Backspace') {
        event.preventDefault();
        deleteSelection();
      } else if (event.key === 'Escape') {
        dismissPanels();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [
    beginEdgeLabelEdit,
    deleteSelection,
    dismissPanels,
    duplicateNode,
    redo,
    selectedEdgeId,
    selectedNodeId,
    undo,
  ]);
}
