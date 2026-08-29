import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

export type FloatingPanelPlacement = "top" | "bottom";
export type FloatingPanelAlign = "start" | "end";

export interface FloatingPanelViewport {
  bottom: number;
  left: number;
  right: number;
  top: number;
}

export interface FloatingPanelPositionOptions {
  align?: FloatingPanelAlign;
  gap?: number;
  maxHeight?: number;
  minWidth?: number;
  viewportMargin?: number;
  width?: number | ((anchorRect: DOMRectReadOnly) => number);
}

export interface FloatingPanelPosition {
  height: number;
  left: number;
  placement: FloatingPanelPlacement;
  top: number;
  visible: boolean;
  width: number;
}

function finite(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
}

/**
 * Calculate a viewport-safe position for a floating panel. Keeping this pure
 * makes the edge conditions easy to verify without a browser layout engine.
 */
export function calculateFloatingPanelPosition(
  anchorRect: DOMRectReadOnly,
  measuredHeight: number,
  viewport: FloatingPanelViewport,
  options: FloatingPanelPositionOptions = {},
): FloatingPanelPosition {
  const gap = Math.max(0, finite(options.gap ?? 6, 6));
  const margin = Math.max(0, finite(options.viewportMargin ?? 8, 8));
  const maxHeight = Math.max(1, finite(options.maxHeight ?? 320, 320));
  const minWidth = Math.max(0, finite(options.minWidth ?? 0, 0));
  const viewportWidth = Math.max(0, viewport.right - viewport.left);
  const viewportHeight = Math.max(0, viewport.bottom - viewport.top);
  const availableWidth = Math.max(1, viewportWidth - margin * 2);
  const requestedWidth =
    typeof options.width === "function"
      ? options.width(anchorRect)
      : (options.width ?? anchorRect.width);
  const width = clamp(
    Math.max(minWidth, finite(requestedWidth, anchorRect.width)),
    1,
    availableWidth,
  );
  const horizontalMinimum = viewport.left + margin;
  const horizontalMaximum = viewport.right - margin - width;
  const preferredLeft =
    options.align === "end" ? anchorRect.right - width : anchorRect.left;
  const left = clamp(preferredLeft, horizontalMinimum, horizontalMaximum);

  const availableBelow = Math.max(
    0,
    viewport.bottom - anchorRect.bottom - gap - margin,
  );
  const availableAbove = Math.max(
    0,
    anchorRect.top - viewport.top - gap - margin,
  );
  const desiredHeight = Math.min(
    maxHeight,
    Math.max(1, finite(measuredHeight, maxHeight)),
  );
  const placement: FloatingPanelPlacement =
    desiredHeight <= availableBelow || availableBelow >= availableAbove
      ? "bottom"
      : "top";
  const availableHeight =
    placement === "bottom" ? availableBelow : availableAbove;
  const height = Math.min(desiredHeight, Math.max(1, availableHeight));
  const preferredTop =
    placement === "bottom"
      ? anchorRect.bottom + gap
      : anchorRect.top - gap - height;
  const verticalMinimum = viewport.top + margin;
  const verticalMaximum = viewport.bottom - margin - height;
  const top = clamp(preferredTop, verticalMinimum, verticalMaximum);
  const anchorVisible =
    anchorRect.right > viewport.left &&
    anchorRect.left < viewport.right &&
    anchorRect.bottom > viewport.top &&
    anchorRect.top < viewport.bottom;

  return {
    height,
    left,
    placement,
    top,
    visible: anchorVisible && availableHeight > 0 && viewportHeight > 0,
    width,
  };
}

function viewportRect(): FloatingPanelViewport {
  const visualViewport = window.visualViewport;
  const left = visualViewport?.offsetLeft ?? 0;
  const top = visualViewport?.offsetTop ?? 0;
  const width = visualViewport?.width || window.innerWidth;
  const height = visualViewport?.height || window.innerHeight;
  return {
    bottom: top + Math.max(0, height),
    left,
    right: left + Math.max(0, width),
    top,
  };
}

function writePosition(
  panel: HTMLElement,
  position: FloatingPanelPosition,
): void {
  panel.style.setProperty("--a3s-floating-left", `${position.left}px`);
  panel.style.setProperty("--a3s-floating-top", `${position.top}px`);
  panel.style.setProperty("--a3s-floating-width", `${position.width}px`);
  panel.style.setProperty("--a3s-floating-max-height", `${position.height}px`);
  panel.dataset.floatingPlacement = position.placement;
  panel.dataset.floatingPositioned = "true";
  panel.dataset.floatingAnchorHidden = position.visible ? "false" : "true";
}

const useIsomorphicLayoutEffect =
  typeof window === "undefined" ? useEffect : useLayoutEffect;

/**
 * Keep an in-panel overlay aligned to its trigger while escaping scroll
 * container clipping through a fixed, viewport-relative position.
 */
export function useFloatingPanelPosition(
  anchorRef: RefObject<HTMLElement | null>,
  panelRef: RefObject<HTMLElement | null>,
  active: boolean,
  options: FloatingPanelPositionOptions = {},
): void {
  const optionsRef = useRef(options);
  optionsRef.current = options;

  useIsomorphicLayoutEffect(() => {
    if (!active || typeof window === "undefined") return;
    const anchor = anchorRef.current;
    const panel = panelRef.current;
    if (!anchor || !panel) return;

    let animationFrame: number | undefined;
    let disposed = false;

    const update = () => {
      if (disposed) return;

      // A select can briefly report its trigger as expanded before the
      // runtime changes aria-hidden on the popover. Wait for that mutation
      // instead of measuring a display:none panel at (0, 0).
      if (
        panel.hasAttribute("aria-hidden") &&
        panel.getAttribute("aria-hidden") !== "false"
      ) {
        return;
      }

      const anchorRect = anchor.getBoundingClientRect();
      const viewport = viewportRect();

      // Remove the previous constrained height before measuring. Otherwise a
      // panel that was clipped above the trigger would stay artificially
      // short after the user scrolls it into a roomier position.
      const currentOptions = optionsRef.current;
      const maxHeight = Math.max(
        1,
        finite(currentOptions.maxHeight ?? 320, 320),
      );
      const margin = Math.max(0, finite(currentOptions.viewportMargin ?? 8, 8));
      const availableWidth = Math.max(
        1,
        viewport.right - viewport.left - margin * 2,
      );
      const requestedWidth =
        typeof currentOptions.width === "function"
          ? currentOptions.width(anchorRect)
          : (currentOptions.width ?? anchorRect.width);
      const provisionalWidth = clamp(
        Math.max(
          0,
          finite(
            Math.max(currentOptions.minWidth ?? 0, requestedWidth),
            anchorRect.width,
          ),
        ),
        1,
        availableWidth,
      );
      panel.dataset.floatingPositioned = "true";
      panel.style.setProperty("--a3s-floating-left", "0px");
      panel.style.setProperty("--a3s-floating-top", "0px");
      panel.style.setProperty("--a3s-floating-width", `${provisionalWidth}px`);
      panel.style.setProperty("--a3s-floating-max-height", `${maxHeight}px`);
      const panelRect = panel.getBoundingClientRect();

      // jsdom and a detached host have no layout metrics. Keep the panel
      // discoverable in that case; a real layout will be positioned on the
      // next resize/scroll/mutation pass.
      if (
        anchorRect.width === 0 &&
        anchorRect.height === 0 &&
        panelRect.width === 0 &&
        panelRect.height === 0
      ) {
        panel.dataset.floatingPositioned = "true";
        panel.dataset.floatingAnchorHidden = "false";
        return;
      }

      const position = calculateFloatingPanelPosition(
        anchorRect,
        panelRect.height,
        viewport,
        optionsRef.current,
      );
      writePosition(panel, position);
    };

    const schedule = () => {
      if (animationFrame !== undefined) return;
      const request = window.requestAnimationFrame;
      if (typeof request === "function") {
        animationFrame = request(() => {
          animationFrame = undefined;
          update();
        });
      } else {
        update();
      }
    };

    const handleViewportChange = () => schedule();
    const handleAttributeChange = () => update();
    const mutationObserver =
      typeof MutationObserver === "function"
        ? new MutationObserver(handleAttributeChange)
        : undefined;
    mutationObserver?.observe(anchor, {
      attributeFilter: ["aria-expanded", "data-open"],
      attributes: true,
    });
    mutationObserver?.observe(panel, {
      attributeFilter: ["aria-hidden"],
      attributes: true,
    });

    window.addEventListener("resize", handleViewportChange, { passive: true });
    window.addEventListener("scroll", handleViewportChange, {
      capture: true,
      passive: true,
    });
    const visualViewport = window.visualViewport;
    visualViewport?.addEventListener("resize", handleViewportChange, {
      passive: true,
    });
    visualViewport?.addEventListener("scroll", handleViewportChange, {
      passive: true,
    });

    let resizeObserver: ResizeObserver | undefined;
    if (typeof ResizeObserver === "function") {
      resizeObserver = new ResizeObserver(schedule);
      resizeObserver.observe(anchor);
      resizeObserver.observe(panel);
    }

    update();
    return () => {
      disposed = true;
      if (animationFrame !== undefined) {
        window.cancelAnimationFrame?.(animationFrame);
      }
      mutationObserver?.disconnect();
      resizeObserver?.disconnect();
      window.removeEventListener("resize", handleViewportChange);
      window.removeEventListener("scroll", handleViewportChange, true);
      visualViewport?.removeEventListener("resize", handleViewportChange);
      visualViewport?.removeEventListener("scroll", handleViewportChange);
      delete panel.dataset.floatingPositioned;
      delete panel.dataset.floatingPlacement;
      delete panel.dataset.floatingAnchorHidden;
      panel.style.removeProperty("--a3s-floating-left");
      panel.style.removeProperty("--a3s-floating-top");
      panel.style.removeProperty("--a3s-floating-width");
      panel.style.removeProperty("--a3s-floating-max-height");
    };
  }, [active, anchorRef, panelRef]);
}
