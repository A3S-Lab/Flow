import { describe, expect, it } from "vitest";
import {
  calculateFloatingPanelPosition,
  type FloatingPanelViewport,
} from "../src/react/floating-panel";

function rect(
  top: number,
  left: number,
  width: number,
  height: number,
): DOMRectReadOnly {
  return {
    bottom: top + height,
    height,
    left,
    right: left + width,
    top,
    width,
    x: left,
    y: top,
    toJSON: () => ({}),
  } as DOMRectReadOnly;
}

const viewport: FloatingPanelViewport = {
  bottom: 600,
  left: 0,
  right: 800,
  top: 0,
};

describe("calculateFloatingPanelPosition", () => {
  it("opens below the trigger when the menu fits", () => {
    const position = calculateFloatingPanelPosition(
      rect(100, 120, 220, 36),
      180,
      viewport,
      { gap: 5, maxHeight: 230, minWidth: 168, viewportMargin: 8 },
    );

    expect(position).toMatchObject({
      height: 180,
      left: 120,
      placement: "bottom",
      top: 141,
      visible: true,
      width: 220,
    });
  });

  it("flips above a trigger near the bottom edge", () => {
    const position = calculateFloatingPanelPosition(
      rect(520, 120, 220, 36),
      180,
      viewport,
      { gap: 5, maxHeight: 230, minWidth: 168, viewportMargin: 8 },
    );

    expect(position).toMatchObject({
      height: 180,
      left: 120,
      placement: "top",
      top: 335,
      visible: true,
      width: 220,
    });
  });

  it("clamps width and height to the viewport while preserving end alignment", () => {
    const position = calculateFloatingPanelPosition(
      rect(260, 720, 140, 36),
      420,
      viewport,
      {
        align: "end",
        gap: 6,
        maxHeight: 420,
        minWidth: 168,
        viewportMargin: 8,
        width: 320,
      },
    );

    expect(position).toMatchObject({
      height: 290,
      left: 472,
      placement: "bottom",
      top: 302,
      visible: true,
      width: 320,
    });
  });

  it("marks a menu hidden when its trigger leaves the viewport", () => {
    const position = calculateFloatingPanelPosition(
      rect(700, 120, 220, 36),
      180,
      viewport,
      { gap: 5, maxHeight: 230, minWidth: 168, viewportMargin: 8 },
    );

    expect(position.visible).toBe(false);
  });
});
