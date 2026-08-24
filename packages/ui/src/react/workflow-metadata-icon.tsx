import { DesignerIcon, type DesignerIconName } from './designer-icons';

const WORKFLOW_ICON_ALIASES: Readonly<Record<string, DesignerIconName>> = {
  'arrow-down': 'arrow-down',
  'arrow-up': 'arrow-up',
  'arrow-up-down': 'list',
  columns: 'columns-3',
  'copy-x': 'copy',
  filter: 'search',
  hash: 'hash',
  'hard-drive': 'desktop',
  'id-card': 'card',
  pencil: 'edit',
  'pencil-line': 'edit',
  replace: 'redo',
  search: 'search',
  sparkles: 'sparkles',
};

function workflowMetadataIconName(value: string): DesignerIconName {
  return WORKFLOW_ICON_ALIASES[value.toLocaleLowerCase()] ?? 'components';
}

export function WorkflowMetadataIcon({ name, size = 13 }: { name: string; size?: number }) {
  return (
    <span title={name}>
      <DesignerIcon name={workflowMetadataIconName(name)} size={size} />
    </span>
  );
}
