import { useId, useState, type KeyboardEvent, type ReactNode } from "react";
import type { A3SFlowDesignerContext } from "../integrations/a3s-flow-designer-context";

export type A3SFlowDesignerExtensionTab = "cli" | "skill" | "copilot";

export type A3SFlowDesignerExtensionSlot =
  ReactNode | ((context: A3SFlowDesignerContext) => ReactNode);

export type A3SFlowDesignerExtensionSlots = Partial<
  Record<A3SFlowDesignerExtensionTab, A3SFlowDesignerExtensionSlot>
>;

export interface A3SFlowDesignerExtensionAreaProps {
  context: A3SFlowDesignerContext;
  extensions?: A3SFlowDesignerExtensionSlots;
  activeTab?: A3SFlowDesignerExtensionTab;
  defaultTab?: A3SFlowDesignerExtensionTab;
  onTabChange?: (tab: A3SFlowDesignerExtensionTab) => void;
  tabLabels?: Partial<Record<A3SFlowDesignerExtensionTab, string>>;
  title?: string;
  ariaLabel?: string;
  className?: string;
  /** Rendered when a host did not provide a slot for the active tab. */
  fallback?: (
    tab: A3SFlowDesignerExtensionTab,
    context: A3SFlowDesignerContext,
  ) => ReactNode;
}

const defaultTabLabels: Record<A3SFlowDesignerExtensionTab, string> = {
  cli: "CLI",
  skill: "Skill",
  copilot: "Copilot",
};

function renderSlot(
  slot: A3SFlowDesignerExtensionSlot | undefined,
  context: A3SFlowDesignerContext,
): ReactNode {
  return typeof slot === "function" ? slot(context) : slot;
}

/**
 * A small, headless-friendly tab area for host-provided Flow integrations.
 *
 * The component owns tab semantics only. It intentionally does not make a
 * network request or mutate the graph; each slot receives the same immutable
 * context and can delegate actions to its host.
 */
export function A3SFlowDesignerExtensionArea({
  context,
  extensions,
  activeTab,
  defaultTab = "copilot",
  onTabChange,
  tabLabels,
  title = "Workflow extensions",
  ariaLabel = "Workflow extensions",
  className,
  fallback,
}: A3SFlowDesignerExtensionAreaProps) {
  const [uncontrolledTab, setUncontrolledTab] =
    useState<A3SFlowDesignerExtensionTab>(defaultTab);
  const currentTab = activeTab ?? uncontrolledTab;
  const tabListId = useId();
  const panelId = `${tabListId}-panel`;
  const labels = { ...defaultTabLabels, ...tabLabels };
  const tabs: A3SFlowDesignerExtensionTab[] = ["cli", "skill", "copilot"];
  const selectTab = (tab: A3SFlowDesignerExtensionTab) => {
    if (activeTab === undefined) setUncontrolledTab(tab);
    onTabChange?.(tab);
  };
  const onTabKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const index = tabs.indexOf(currentTab);
    let nextIndex: number | undefined;
    if (event.key === "ArrowLeft")
      nextIndex = (index + tabs.length - 1) % tabs.length;
    if (event.key === "ArrowRight") nextIndex = (index + 1) % tabs.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = tabs.length - 1;
    if (nextIndex === undefined) return;
    event.preventDefault();
    const next = tabs[nextIndex];
    selectTab(next);
    document.getElementById(`${tabListId}-${next}`)?.focus();
  };
  const supplied = extensions?.[currentTab];
  const content =
    supplied !== undefined ? (
      renderSlot(supplied, context)
    ) : (
      fallback?.(currentTab, context) ?? (
        <pre data-a3s-flow-designer-context="fallback">
          {context.documentJson}
        </pre>
      )
    );

  return (
    <section
      aria-label={ariaLabel}
      className={["a3s-flow-designer-extensions", className]
        .filter(Boolean)
        .join(" ")}
      data-a3s-flow-designer-extensions=""
    >
      <header className="a3s-flow-designer-extensions__header">
        <strong>{title}</strong>
        <div
          aria-label={ariaLabel}
          className="a3s-flow-designer-extensions__tabs"
          id={tabListId}
          role="tablist"
        >
          {tabs.map((tab) => (
            <button
              aria-controls={panelId}
              aria-selected={currentTab === tab}
              className={currentTab === tab ? "is-active" : undefined}
              id={`${tabListId}-${tab}`}
              key={tab}
              onClick={() => selectTab(tab)}
              onKeyDown={onTabKeyDown}
              role="tab"
              tabIndex={currentTab === tab ? 0 : -1}
              type="button"
            >
              {labels[tab]}
            </button>
          ))}
        </div>
      </header>
      <div
        aria-labelledby={`${tabListId}-${currentTab}`}
        className="a3s-flow-designer-extensions__panel"
        id={panelId}
        role="tabpanel"
        tabIndex={0}
      >
        {content}
      </div>
    </section>
  );
}

export type { A3SFlowDesignerContext } from "../integrations/a3s-flow-designer-context";
