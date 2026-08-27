import { Plus } from '@phosphor-icons/react';
import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  getSmoothStepPath,
  ViewportPortal,
  type EdgeProps,
} from '@xyflow/react';
import { memo } from 'react';
import type { PlaygroundEdge } from './WorkflowPlayground.model';

type WorkflowPlaygroundEdgeLabelProps = {
  data?: PlaygroundEdge['data'];
  id: string;
  internal?: boolean;
  labelX: number;
  labelY: number;
};

function WorkflowPlaygroundEdgeLabel({
  data,
  id,
  internal = false,
  labelX,
  labelY,
}: WorkflowPlaygroundEdgeLabelProps) {
  return (
    <div
      className={`a3s-workflow-edge-label nodrag nopan${internal ? ' is-internal' : ''}`}
      style={{
        transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
      }}
    >
      {data?.sourcePortLabel && <span>{data.sourcePortLabel}</span>}
      {data?.onInsert && (
        <button
          aria-label={data.insertLabel}
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
}

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
            height="1"
            overflow="visible"
            width="1"
          >
            <path
              className={`react-flow__edge-path a3s-workflow-internal-edge-path${selected ? ' is-selected' : ''}${animated ? ' is-animated' : ''}`}
              d={path}
              markerEnd={markerEnd}
              style={{ ...style, pointerEvents: 'none' }}
            />
          </svg>
          <WorkflowPlaygroundEdgeLabel
            data={data}
            id={id}
            internal
            labelX={labelX}
            labelY={labelY}
          />
        </ViewportPortal>
      ) : (
        <EdgeLabelRenderer>
          <WorkflowPlaygroundEdgeLabel
            data={data}
            id={id}
            labelX={labelX}
            labelY={labelY}
          />
        </EdgeLabelRenderer>
      )}
    </>
  );
}

export const WorkflowPlaygroundEdge = memo(WorkflowPlaygroundEdgeComponent);
