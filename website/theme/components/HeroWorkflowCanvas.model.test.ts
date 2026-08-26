import { describe, expect, it } from 'vitest';
import {
  clampHeroOffset,
  heroEdgePath,
  heroTransformFromTransform,
} from './HeroWorkflowCanvas.model';

describe('homepage workflow canvas geometry', () => {
  it('builds a compact cubic edge path with stable precision', () => {
    expect(heroEdgePath({ x: 12, y: 18 }, { x: 58, y: 64 })).toBe(
      'M 12.00 18.00 C 30.00 18.00, 76.00 64.00, 58.00 64.00',
    );
  });

  it('reads scale and translation from 2d and 3d CSS matrices', () => {
    expect(
      heroTransformFromTransform('matrix(1.25, 0, 0, 1.25, 18, -6)', 1),
    ).toEqual({
      scale: 1.25,
      x: 18,
      y: -6,
    });
    expect(
      heroTransformFromTransform(
        'matrix3d(0.8, 0, 0, 0, 0, 0.8, 0, 0, 0, 0, 1, 0, 24, 12, 0, 1)',
        1,
      ),
    ).toEqual({ scale: 0.8, x: 24, y: 12 });
  });

  it('falls back safely and clamps pointer offsets', () => {
    expect(heroTransformFromTransform('none', 0.82)).toEqual({
      scale: 0.82,
      x: 0,
      y: 0,
    });
    expect(heroTransformFromTransform('invalid', 0.82)).toEqual({
      scale: 0.82,
      x: 0,
      y: 0,
    });
    expect(clampHeroOffset(180, 96)).toBe(96);
    expect(clampHeroOffset(-180, 96)).toBe(-96);
  });
});
