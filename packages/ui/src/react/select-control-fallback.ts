import {
  useCallback,
  type KeyboardEventHandler,
  type MouseEventHandler,
  type MutableRefObject,
} from 'react';

export type SelectElement = HTMLElement & {
  _destroy?: () => void;
  refresh?: () => void;
  close?: (focusOnTrigger?: boolean) => void;
  open?: () => void;
  value?: string;
};

export type SelectOption = {
  disabled: boolean;
  label: string;
  value: string;
};

export type SelectControlChangeEvent = {
  currentTarget: { value: string };
  target: { value: string };
};

export function hasSelectRuntime(element: SelectElement | null): boolean {
  return (
    typeof element?.togglePopover === 'function' &&
    typeof element.open === 'function'
  );
}

type SelectFallbackControllerOptions = {
  elementRef: MutableRefObject<SelectElement | null>;
  optionsRef: MutableRefObject<readonly SelectOption[]>;
  valueRef: MutableRefObject<string>;
  disabledRef: MutableRefObject<boolean>;
  fallbackOpenRef: MutableRefObject<boolean>;
  fallbackActiveIndexRef: MutableRefObject<number>;
  onChangeRef: MutableRefObject<
    ((event: SelectControlChangeEvent) => void) | undefined
  >;
  ensureRuntime: () => Promise<void>;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  onKeyDown?: KeyboardEventHandler<HTMLButtonElement>;
};

type SelectFallbackController = {
  setFallbackOpen: (open: boolean) => void;
  handleTriggerClick: MouseEventHandler<HTMLButtonElement>;
  handleTriggerKeyDown: KeyboardEventHandler<HTMLButtonElement>;
  handleFallbackListboxClick: MouseEventHandler<HTMLDivElement>;
};

/**
 * Provides a small progressive-enhancement fallback while the A3S UI Select
 * runtime is loading. The runtime remains authoritative once its methods are
 * attached; this controller only mirrors the same ARIA and value state while
 * that asynchronous boundary is in flight.
 */
export function useSelectFallbackController({
  elementRef,
  optionsRef,
  valueRef,
  disabledRef,
  fallbackOpenRef,
  fallbackActiveIndexRef,
  onChangeRef,
  ensureRuntime,
  onClick,
  onKeyDown,
}: SelectFallbackControllerOptions): SelectFallbackController {
  const setFallbackActive = useCallback(
    (index: number) => {
      const element = elementRef.current;
      if (!element) return;
      const popover = element.querySelector<HTMLElement>(
        ':scope > [data-popover]',
      );
      const trigger =
        element.querySelector<HTMLButtonElement>(':scope > button');
      if (!popover || !trigger) return;

      const optionElements = Array.from(
        popover.querySelectorAll<HTMLElement>('[role="option"]'),
      );
      const option = optionsRef.current[index];
      const activeIndex =
        option &&
        !option.disabled &&
        index >= 0 &&
        index < optionElements.length
          ? index
          : -1;

      optionElements.forEach((candidate, candidateIndex) => {
        candidate.classList.toggle('active', candidateIndex === activeIndex);
      });
      fallbackActiveIndexRef.current = activeIndex;
      const activeOption =
        activeIndex >= 0 ? optionElements[activeIndex] : undefined;
      if (activeOption?.id) {
        trigger.setAttribute('aria-activedescendant', activeOption.id);
      } else {
        trigger.removeAttribute('aria-activedescendant');
      }
    },
    [elementRef, fallbackActiveIndexRef, optionsRef],
  );

  const setFallbackOpen = useCallback(
    (open: boolean) => {
      const element = elementRef.current;
      if (!element) return;
      const popover = element.querySelector<HTMLElement>(
        ':scope > [data-popover]',
      );
      const trigger =
        element.querySelector<HTMLButtonElement>(':scope > button');
      if (!popover || !trigger) return;

      fallbackOpenRef.current = open;
      popover.setAttribute('aria-hidden', String(!open));
      trigger.setAttribute('aria-expanded', String(open));
      if (!open) {
        setFallbackActive(-1);
        return;
      }

      const selectedIndex = optionsRef.current.findIndex(
        (option) => !option.disabled && option.value === valueRef.current,
      );
      setFallbackActive(selectedIndex);
    },
    [elementRef, fallbackOpenRef, optionsRef, setFallbackActive, valueRef],
  );

  const selectFallbackOption = useCallback(
    (optionElement: HTMLElement) => {
      const element = elementRef.current;
      if (!element || disabledRef.current) return;
      const popover = element.querySelector<HTMLElement>(
        ':scope > [data-popover]',
      );
      const trigger =
        element.querySelector<HTMLButtonElement>(':scope > button');
      if (!popover || !trigger) return;

      const optionElements = Array.from(
        popover.querySelectorAll<HTMLElement>('[role="option"]'),
      );
      const index = optionElements.indexOf(optionElement);
      const option = optionsRef.current[index];
      if (!option || option.disabled) return;

      const input = element.querySelector<HTMLInputElement>(
        ':scope > input[type="hidden"]',
      );
      const currentValue = input?.value ?? valueRef.current;
      optionElements.forEach((candidate, candidateIndex) => {
        candidate.setAttribute(
          'aria-selected',
          String(candidateIndex === index),
        );
      });
      if (input) input.value = option.value;
      element.dataset.valueEmpty = option.value === '' ? 'true' : 'false';
      const label = trigger.querySelector<HTMLElement>(':scope > span');
      if (label) label.textContent = option.label;
      setFallbackOpen(false);

      if (currentValue === option.value) return;
      onChangeRef.current?.({
        currentTarget: { value: option.value },
        target: { value: option.value },
      });
    },
    [
      disabledRef,
      elementRef,
      onChangeRef,
      optionsRef,
      setFallbackOpen,
      valueRef,
    ],
  );

  const moveFallbackActive = useCallback(
    (direction: 'first' | 'last' | 'next' | 'previous') => {
      const currentIndex = fallbackActiveIndexRef.current;
      const available = optionsRef.current
        .map((option, index) => (option.disabled ? -1 : index))
        .filter((index) => index >= 0);
      if (available.length === 0) return;
      let nextIndex = available[0];
      if (direction === 'last') nextIndex = available[available.length - 1];
      if (direction === 'next') {
        nextIndex =
          available.find((index) => index > currentIndex) ??
          available[available.length - 1];
      }
      if (direction === 'previous') {
        nextIndex =
          [...available].reverse().find((index) => index < currentIndex) ??
          available[0];
      }
      setFallbackActive(nextIndex);
    },
    [fallbackActiveIndexRef, optionsRef, setFallbackActive],
  );

  const handleTriggerClick: MouseEventHandler<HTMLButtonElement> = (event) => {
    onClick?.(event);
    if (event.defaultPrevented || disabledRef.current) return;
    const element = elementRef.current;
    if (!element) return;

    // The native runtime listener runs before React's delegated listener. If
    // it is present, let it own the toggle and only mirror its state locally.
    if (hasSelectRuntime(element)) {
      fallbackOpenRef.current =
        element
          .querySelector<HTMLButtonElement>(':scope > button')
          ?.getAttribute('aria-expanded') === 'true';
      return;
    }

    const trigger = element.querySelector<HTMLButtonElement>(':scope > button');
    const open =
      fallbackOpenRef.current ||
      trigger?.getAttribute('aria-expanded') === 'true';
    setFallbackOpen(!open);
    if (!open) void ensureRuntime();
  };

  const handleTriggerKeyDown: KeyboardEventHandler<HTMLButtonElement> = (
    event,
  ) => {
    onKeyDown?.(event);
    if (event.defaultPrevented || disabledRef.current) return;
    const element = elementRef.current;
    if (!element) return;

    if (hasSelectRuntime(element)) {
      fallbackOpenRef.current =
        element
          .querySelector<HTMLButtonElement>(':scope > button')
          ?.getAttribute('aria-expanded') === 'true';
      return;
    }

    const trigger = element.querySelector<HTMLButtonElement>(':scope > button');
    const open =
      fallbackOpenRef.current ||
      trigger?.getAttribute('aria-expanded') === 'true';
    const key = event.key === 'Spacebar' ? ' ' : event.key;
    if (
      !['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter', ' ', 'Escape'].includes(
        key,
      )
    ) {
      return;
    }

    event.preventDefault();
    if (key === 'Escape') {
      if (open) setFallbackOpen(false);
      return;
    }
    if (!open) {
      setFallbackOpen(true);
      void ensureRuntime();
      return;
    }
    if (key === 'Enter' || key === ' ') {
      const popover = element.querySelector<HTMLElement>(
        ':scope > [data-popover]',
      );
      const option =
        popover?.querySelectorAll<HTMLElement>('[role="option"]')[
          fallbackActiveIndexRef.current
        ];
      if (option) selectFallbackOption(option);
      return;
    }
    if (key === 'ArrowDown') moveFallbackActive('next');
    if (key === 'ArrowUp') moveFallbackActive('previous');
    if (key === 'Home') moveFallbackActive('first');
    if (key === 'End') moveFallbackActive('last');
  };

  const handleFallbackListboxClick: MouseEventHandler<HTMLDivElement> = (
    event,
  ) => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const option = target.closest<HTMLElement>('[role="option"]');
    if (!option) return;

    const element = elementRef.current;
    if (!element) return;
    const optionIndex = Array.from(
      element.querySelectorAll<HTMLElement>(
        ':scope > [data-popover] [role="option"]',
      ),
    ).indexOf(option);
    const expectedValue = optionsRef.current[optionIndex]?.value;
    if (hasSelectRuntime(element)) {
      // The runtime's direct listener normally handles this click first. If a
      // listener was attached during the same event, process only when it did
      // not actually update the value yet.
      const currentValue = element.querySelector<HTMLInputElement>(
        ':scope > input[type="hidden"]',
      )?.value;
      if (!fallbackOpenRef.current || currentValue === expectedValue) {
        fallbackOpenRef.current = false;
        return;
      }
    }
    selectFallbackOption(option);
  };

  return {
    setFallbackOpen,
    handleTriggerClick,
    handleTriggerKeyDown,
    handleFallbackListboxClick,
  };
}
