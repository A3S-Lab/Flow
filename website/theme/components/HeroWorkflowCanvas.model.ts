import { a3sFlowDagNodeRegistry, createA3SFlowDagNode } from '@a3s-lab/flow-ui';
import { GitBranch, Play, Wrench } from '@phosphor-icons/react';
import type { HomeLocale } from './HomeCopy';

export type NodeConfiguration = NonNullable<
  Parameters<typeof createA3SFlowDagNode>[2]
>;

export function demoNode(
  type: string,
  id: string,
  configuration: NodeConfiguration = {},
) {
  return createA3SFlowDagNode(
    id,
    a3sFlowDagNodeRegistry.require(type),
    configuration,
    { position: { x: 0, y: 0 } },
  );
}

export type HeroRunState = 'idle' | 'running' | 'success';
export type HeroTool = 'select' | 'pan' | 'add';
export type HeroInspectorTab = 'settings' | 'run';
export type HeroNodeStatus = 'idle' | 'running' | 'success';
export type HeroGraphPoint = { x: number; y: number };
export type HeroEdgePaths = { first: string; second: string };
export type HeroPointOffset = { x: number; y: number };
export type HeroGraphTransform = { scale: number; x: number; y: number };
export type HeroPointerGesture = {
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  targetId?: string;
  moved: boolean;
};

export const heroNodeIcons = [Play, Wrench, GitBranch] as const;

export function heroEdgePath(
  start: HeroGraphPoint,
  end: HeroGraphPoint,
): string {
  const lead = Math.max(8, Math.min(18, Math.abs(end.x - start.x) * 0.28 + 6));
  const controlStart = { x: start.x + lead, y: start.y };
  const controlEnd = { x: end.x + lead, y: end.y };

  return [
    `M ${start.x.toFixed(2)} ${start.y.toFixed(2)}`,
    `C ${controlStart.x.toFixed(2)} ${controlStart.y.toFixed(2)},`,
    `${controlEnd.x.toFixed(2)} ${controlEnd.y.toFixed(2)},`,
    `${end.x.toFixed(2)} ${end.y.toFixed(2)}`,
  ].join(' ');
}

export function heroTransformFromTransform(
  transform: string,
  fallbackScale: number,
): HeroGraphTransform {
  if (transform === 'none') {
    return { scale: fallbackScale, x: 0, y: 0 };
  }

  const open = transform.indexOf('(');
  const close = transform.lastIndexOf(')');
  if (open < 0 || close <= open) {
    return { scale: fallbackScale, x: 0, y: 0 };
  }

  const values = transform
    .slice(open + 1, close)
    .split(',')
    .map((value) => Number.parseFloat(value.trim()));
  const is3d = transform.startsWith('matrix3d');
  const scale = values[0];
  const x = values[is3d ? 12 : 4];
  const y = values[is3d ? 13 : 5];

  return {
    scale: Number.isFinite(scale) && scale > 0 ? scale : fallbackScale,
    x: Number.isFinite(x) ? x : 0,
    y: Number.isFinite(y) ? y : 0,
  };
}

export function clampHeroOffset(value: number, limit: number): number {
  return Math.max(-limit, Math.min(limit, value));
}

export function readableManifestValue(
  value: unknown,
  locale: HomeLocale,
): string {
  if (typeof value === 'string' && value.trim()) return value;
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }
  return locale === 'zh' ? '已连接' : 'Connected';
}

export type HeroDemoNode = ReturnType<typeof demoNode>;
