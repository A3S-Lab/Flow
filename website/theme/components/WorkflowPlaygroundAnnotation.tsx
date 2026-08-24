import { ChatCircleDots, NotePencil, Trash } from '@phosphor-icons/react';
import type { NodeProps } from '@xyflow/react';
import { memo } from 'react';
import type { PlaygroundAnnotationNode } from './WorkflowPlayground.model';

function WorkflowPlaygroundAnnotationComponent({
  id,
  data,
}: NodeProps<PlaygroundAnnotationNode>) {
  const Icon = data.kind === 'note' ? NotePencil : ChatCircleDots;

  return (
    <article
      className="a3s-workflow-annotation"
      data-kind={data.kind}
      data-testid={`workflow-${data.kind}`}
    >
      <header>
        <span>
          <Icon aria-hidden="true" weight="fill" />
          <strong>{data.label}</strong>
        </span>
        <button
          aria-label={data.deleteLabel}
          className="nodrag nopan"
          onClick={(event) => {
            event.stopPropagation();
            data.onDelete?.(id);
          }}
          title={data.deleteLabel}
          type="button"
        >
          <Trash aria-hidden="true" />
        </button>
      </header>
      <textarea
        aria-label={data.label}
        className="nodrag nopan nowheel"
        onBlur={data.onEditEnd}
        onChange={(event) => data.onTextChange?.(id, event.target.value)}
        onFocus={data.onEditStart}
        onKeyDown={(event) => {
          if (event.key === 'Escape') event.currentTarget.blur();
        }}
        placeholder={data.placeholder}
        rows={data.kind === 'note' ? 5 : 3}
        value={data.text}
      />
    </article>
  );
}

export const WorkflowPlaygroundAnnotation = memo(
  WorkflowPlaygroundAnnotationComponent,
);
