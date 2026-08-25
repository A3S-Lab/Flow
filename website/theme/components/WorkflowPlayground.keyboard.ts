import { useEffect } from 'react';

type WorkflowPlaygroundKeyboardOptions = {
  deleteSelection: () => void;
  dismissPanels: () => void;
  duplicateNode: (nodeId: string) => void;
  redo: () => void;
  selectedNodeId?: string;
  undo: () => void;
};

export function useWorkflowPlaygroundKeyboard({
  deleteSelection,
  dismissPanels,
  duplicateNode,
  redo,
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
    deleteSelection,
    dismissPanels,
    duplicateNode,
    redo,
    selectedNodeId,
    undo,
  ]);
}
