import { Plus } from '@phosphor-icons/react';
import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  getSmoothStepPath,
  type EdgeProps,
} from '@xyflow/react';
import { memo } from 'react';
import type { PlaygroundEdge } from './WorkflowPlayground.model';

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
}: EdgeProps<PlaygroundEdge>) {
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

  return (
    <>
      <BaseEdge
        className={selected ? 'is-selected' : undefined}
        id={id}
        markerEnd={markerEnd}
        path={path}
        style={style}
      />
      <EdgeLabelRenderer>
        <div
          className="a3s-workflow-edge-label nodrag nopan"
          style={{
            transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
          }}
        >
          {data?.sourcePortLabel && <span>{data.sourcePortLabel}</span>}
          {data?.onInsert && (
            <button
              aria-label={data.insertLabel}
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
      </EdgeLabelRenderer>
    </>
  );
}

export const WorkflowPlaygroundEdge = memo(WorkflowPlaygroundEdgeComponent);
