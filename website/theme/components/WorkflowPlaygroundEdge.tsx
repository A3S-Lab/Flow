import { PencilSimple, Plus } from '@phosphor-icons/react';
import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  getSmoothStepPath,
  ViewportPortal,
  type EdgeProps,
} from '@xyflow/react';
import { memo, useEffect, useRef, useState } from 'react';
import {
  normalizePlaygroundEdgeLabel,
  PLAYGROUND_EDGE_LABEL_MAX_LENGTH,
  type PlaygroundEdge,
} from './WorkflowPlayground.model';

function WorkflowPlaygroundEdgeComponent({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  markerEnd,
  style,
  data,
  selected,
  animated,
}: EdgeProps<PlaygroundEdge>) {
  const displayLabel =
    normalizePlaygroundEdgeLabel(data?.labelOverride) ?? data?.sourcePortLabel;
  const editing = data?.editingLabel === true;
  const [draft, setDraft] = useState(displayLabel ?? '');
  const inputRef = useRef<HTMLInputElement>(null);
  const editingRef = useRef(false);

  useEffect(() => {
    if (!editing) {
      editingRef.current = false;
      return;
    }
    editingRef.current = true;
    setDraft(displayLabel ?? '');
    const focusInput = () => {
      inputRef.current?.focus();
      inputRef.current?.select();
    };
    const frame =
      typeof window === 'undefined'
        ? undefined
        : window.requestAnimationFrame(focusInput);
    return () => {
      if (frame !== undefined) window.cancelAnimationFrame(frame);
    };
  }, [editing, id]);

  const selectEdge = (event: React.SyntheticEvent) => {
    event.stopPropagation();
    data?.onSelect?.(id);
  };
  const startEditing = (event: React.SyntheticEvent) => {
    event.stopPropagation();
    data?.onSelect?.(id);
    data?.onEditLabel?.(id);
  };
  const finishEditing = (save: boolean) => {
    if (!editingRef.current) return;
    editingRef.current = false;
    if (save) data?.onCommitLabel?.(id, draft);
    else data?.onCancelLabel?.(id);
  };

  const pathOptions = {
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  };
  const [path, labelX, labelY] =
    data?.routing === 'orthogonal'
      ? getSmoothStepPath({ ...pathOptions, borderRadius: 18, offset: 28 })
      : getBezierPath({ ...pathOptions, curvature: 0.25 });
  const internal = data?.internal === true;

  const label = (
    <div
      className={`a3s-workflow-edge-label nodrag nopan${internal ? ' is-internal' : ''}${selected ? ' is-selected' : ''}${editing ? ' is-editing' : ''}`}
      data-edge-id={id}
      style={{
        transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
      }}
    >
      {(displayLabel || editing || (selected && data?.onEditLabel)) &&
        (editing ? (
          <input
            aria-label={data?.editLabel}
            autoComplete="off"
            maxLength={PLAYGROUND_EDGE_LABEL_MAX_LENGTH}
            onBlur={() => finishEditing(true)}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                event.stopPropagation();
                finishEditing(true);
              } else if (event.key === 'Escape') {
                event.preventDefault();
                event.stopPropagation();
                finishEditing(false);
              }
            }}
            onMouseDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
            placeholder={data?.labelPlaceholder}
            ref={inputRef}
            spellCheck={false}
            type="text"
            value={draft}
          />
        ) : (
          <button
            aria-label={data?.editLabel}
            className="a3s-workflow-edge-label__text"
            onClick={selectEdge}
            onDoubleClick={startEditing}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === 'F2') {
                event.preventDefault();
                startEditing(event);
              }
            }}
            onMouseDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
            title={data?.editLabel}
            type="button"
          >
            {displayLabel ?? data?.labelPlaceholder}
          </button>
        ))}
      {!editing && data?.onEditLabel && (displayLabel || selected) && (
        <button
          aria-label={data.editLabel}
          className="a3s-workflow-edge-label__edit"
          onClick={startEditing}
          onMouseDown={(event) => event.stopPropagation()}
          onPointerDown={(event) => event.stopPropagation()}
          title={data.editLabel}
          type="button"
        >
          <PencilSimple aria-hidden="true" weight="bold" />
        </button>
      )}
      {data?.onInsert && (
        <button
          aria-label={data.insertLabel}
          className="a3s-workflow-edge-label__insert"
          onMouseDown={(event) => event.stopPropagation()}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            data.onInsert?.(id, { x: labelX, y: labelY });
          }}
          title={data.insertLabel}
          type="button"
        >
          <Plus aria-hidden="true" weight="bold" />
        </button>
      )}
    </div>
  );

  return (
    <>
      <BaseEdge
        className={
          [
            selected ? 'is-selected' : '',
            internal ? 'is-internal-underlay' : '',
          ]
            .filter(Boolean)
            .join(' ') || undefined
        }
        id={id}
        markerEnd={markerEnd}
        path={path}
        style={style}
      />
      {internal ? (
        <ViewportPortal>
          <svg
            aria-hidden="true"
            className="a3s-workflow-internal-edge-overlay"
            data-edge-id={id}
            overflow="visible"
            preserveAspectRatio="none"
          >
            <path
              className={`react-flow__edge-path a3s-workflow-internal-edge-path${selected ? ' is-selected' : ''}${animated ? ' is-animated' : ''}`}
              d={path}
              markerEnd={markerEnd}
              style={{ ...style, pointerEvents: 'none' }}
            />
          </svg>
          {label}
        </ViewportPortal>
      ) : (
        <EdgeLabelRenderer>{label}</EdgeLabelRenderer>
      )}
    </>
  );
}

export const WorkflowPlaygroundEdge = memo(WorkflowPlaygroundEdgeComponent);
