import {
  Children,
  isValidElement,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  type AriaAttributes,
  type KeyboardEventHandler,
  type MouseEventHandler,
  type ReactNode,
  type SelectHTMLAttributes,
} from 'react';
import { loadA3SUIRuntime } from './a3s-ui-runtime';
import { DesignerIcon } from './designer-icons';
import {
  hasSelectRuntime,
  type SelectControlChangeEvent,
  type SelectElement,
  type SelectOption,
  useSelectFallbackController,
} from './select-control-fallback';

function loadSelectRuntime(): Promise<void> {
  return loadA3SUIRuntime('select', () => import('@a3s-lab/ui/select'));
}

type SelectRuntimeStatus = 'idle' | 'loading' | 'ready' | 'failed';

export type { SelectControlChangeEvent } from './select-control-fallback';

export interface SelectControlProps extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  | 'children'
  | 'defaultValue'
  | 'multiple'
  | 'onBlur'
  | 'onChange'
  | 'onFocus'
  | 'onClick'
  | 'onKeyDown'
  | 'value'
> {
  [key: `data-${string}`]: string | number | boolean | undefined;
  /** Optional id for the actual focusable trigger button. */
  triggerId?: string;
  children: ReactNode;
  onKeyDown?: KeyboardEventHandler<HTMLButtonElement>;
  onClick?: MouseEventHandler<HTMLButtonElement>;
  onBlur?: () => void;
  onChange?: (event: SelectControlChangeEvent) => void;
  onFocus?: () => void;
  /** Pass `null` to disable empty-option placeholder inference. */
  placeholder?: string | null;
  value?: string | number | null;
}

function selectOptions(
  children: ReactNode,
  inheritedDisabled = false,
): SelectOption[] {
  return Children.toArray(children).flatMap((child) => {
    if (
      !isValidElement<{
        children?: ReactNode;
        disabled?: boolean;
        label?: ReactNode;
        value?: unknown;
      }>(child)
    ) {
      return [];
    }
    if (child.type === 'optgroup') {
      return selectOptions(
        child.props.children,
        inheritedDisabled || Boolean(child.props.disabled),
      );
    }
    if (child.type !== 'option') return [];
    const value = String(child.props.value ?? '');
    const label =
      optionText(child.props.children) || optionText(child.props.label);
    return [
      {
        disabled: inheritedDisabled || Boolean(child.props.disabled),
        label,
        value,
      },
    ];
  });
}

/**
 * React option labels may contain fragments and lightweight icon elements.
 * The runtime only needs readable text, so avoid the `[object Object]` output
 * produced by Array#join for nested children.
 */
function optionText(children: ReactNode, trim = true): string {
  let text = '';
  Children.forEach(children, (child) => {
    if (
      typeof child === 'string' ||
      typeof child === 'number' ||
      typeof child === 'bigint'
    ) {
      text += String(child);
      return;
    }
    if (isValidElement<{ children?: ReactNode }>(child)) {
      text += optionText(child.props.children, false);
    }
  });
  return trim ? text.trim() : text;
}

function splitAdditionalAttributes(attributes: Record<string, unknown>): {
  root: Record<string, unknown>;
  trigger: Record<string, unknown>;
} {
  const root: Record<string, unknown> = {};
  const trigger: Record<string, unknown> = {};
  for (const [key, attribute] of Object.entries(attributes)) {
    if (attribute === undefined) continue;
    if (
      key.startsWith('data-') ||
      key === 'dir' ||
      key === 'lang' ||
      key === 'style'
    ) {
      root[key] = attribute;
    } else if (key.startsWith('aria-')) {
      trigger[key] = attribute;
    }
  }
  return { root, trigger };
}

/**
 * Controlled A3S UI Select adapter used by Flow composite widgets.
 * It preserves the familiar `event.target.value` callback without falling
 * back to the browser-native select surface.
 */
export function SelectControl({
  'aria-describedby': ariaDescribedBy,
  'aria-disabled': ariaDisabled,
  'aria-invalid': ariaInvalid,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  children,
  className,
  disabled = false,
  id: suppliedId,
  name,
  onBlur,
  onClick,
  onChange,
  onFocus,
  onKeyDown,
  placeholder: suppliedPlaceholder,
  required,
  tabIndex,
  title,
  triggerId: triggerIdProp,
  value,
  ...additionalAttributes
}: SelectControlProps) {
  const generatedId = useId();
  const generatedRootId = `a3s-flow-select-${generatedId.replaceAll(':', '')}`;
  const suppliedRootId = suppliedId?.trim() || undefined;
  const suppliedTriggerId = triggerIdProp?.trim() || undefined;
  // FormRenderer labels target their field id. When a caller asks for that
  // same id on the trigger, move the non-focusable runtime root to a stable
  // sibling id so the DOM never contains duplicate ids.
  const id =
    suppliedRootId && suppliedTriggerId === suppliedRootId
      ? `${suppliedRootId}-root`
      : (suppliedRootId ?? generatedRootId);
  const triggerId = suppliedTriggerId ?? `${id}-trigger`;
  const disabledState =
    disabled || ariaDisabled === true || ariaDisabled === 'true';
  const { root: rootAttributes, trigger: triggerAttributes } =
    splitAdditionalAttributes(additionalAttributes);
  const options = useMemo(() => selectOptions(children), [children]);
  const optionsSignature = JSON.stringify(options);
  const emptyOption = options.find((option) => option.value === '');
  // An explicit placeholder always wins. For backwards compatibility with
  // native-select children, infer a placeholder only from a first, labelled
  // empty option; an empty value elsewhere remains a legitimate option.
  const inferredPlaceholder =
    suppliedPlaceholder !== null &&
    suppliedPlaceholder === undefined &&
    options[0]?.value === '' &&
    Boolean(emptyOption?.label);
  const hasPlaceholder =
    suppliedPlaceholder !== null &&
    (suppliedPlaceholder !== undefined || inferredPlaceholder);
  const placeholder =
    suppliedPlaceholder ??
    (inferredPlaceholder ? emptyOption?.label : '') ??
    '';
  const placeholderOption = hasPlaceholder ? emptyOption : undefined;
  const normalizedValue =
    value === null || value === undefined ? '' : String(value);
  const matched = options.find(
    (option) =>
      option.value === normalizedValue &&
      !option.disabled &&
      !(hasPlaceholder && normalizedValue === ''),
  );
  const firstEnabled = options.find((option) => !option.disabled);
  const selected =
    matched ??
    (hasPlaceholder
      ? placeholderOption && !placeholderOption.disabled
        ? placeholderOption
        : undefined
      : firstEnabled);
  // Keep the hidden value and the runtime's visible value aligned when a
  // stale configuration points at an option that no longer exists. Native
  // selects expose an empty value when a placeholder exists, otherwise they
  // settle on the first enabled option; the custom control does the same.
  const effectiveValue = matched
    ? normalizedValue
    : hasPlaceholder
      ? ''
      : (firstEnabled?.value ?? '');
  const selectedLabel = matched
    ? matched.label
    : hasPlaceholder
      ? placeholder
      : (selected?.label ?? '');
  const elementRef = useRef<SelectElement | null>(null);
  const valueRef = useRef(effectiveValue);
  const onChangeRef = useRef(onChange);
  const optionsRef = useRef(options);
  const disabledRef = useRef(disabledState);
  const mountedRef = useRef(false);
  const fallbackOpenRef = useRef(false);
  const fallbackActiveIndexRef = useRef(-1);
  const runtimeStatusRef = useRef<SelectRuntimeStatus>('idle');
  const runtimePromiseRef = useRef<Promise<void> | null>(null);
  valueRef.current = effectiveValue;
  onChangeRef.current = onChange;
  optionsRef.current = options;
  disabledRef.current = disabledState;

  const synchronize = useCallback((element: HTMLElement) => {
    const select = element as SelectElement;
    select.refresh?.();
    if (select.refresh && select.value !== valueRef.current) {
      select.value = valueRef.current;
    }
    element.dataset.valueEmpty = valueRef.current === '' ? 'true' : 'false';
  }, []);

  const ensureRuntime = useCallback((): Promise<void> => {
    const element = elementRef.current;
    if (!element) return Promise.resolve();

    if (hasSelectRuntime(element)) {
      runtimeStatusRef.current = 'ready';
      synchronize(element);
      return Promise.resolve();
    }

    if (runtimeStatusRef.current === 'loading' && runtimePromiseRef.current) {
      return runtimePromiseRef.current;
    }

    runtimeStatusRef.current = 'loading';
    const request = loadSelectRuntime()
      .then(() => {
        if (!mountedRef.current) return;
        const current = elementRef.current;
        if (!current) return;

        // A host may have loaded the component module before this element was
        // inserted. Ask the shared runtime to scan once more in that case.
        if (!hasSelectRuntime(current)) {
          window.basecoat?.init('select');
        }
        if (!hasSelectRuntime(current)) {
          runtimeStatusRef.current = 'failed';
          return;
        }

        runtimeStatusRef.current = 'ready';
        synchronize(current);
        if (fallbackOpenRef.current && !disabledRef.current) {
          current.open?.();
        } else if (disabledRef.current) {
          fallbackOpenRef.current = false;
          current.close?.(false);
        }
      })
      .catch((error: unknown) => {
        runtimeStatusRef.current = 'failed';
        if (mountedRef.current && typeof window !== 'undefined') {
          console.error(
            '[A3S Flow] Failed to initialize the select control.',
            error,
          );
        }
      });
    runtimePromiseRef.current = request;
    return request;
  }, [synchronize]);

  const {
    setFallbackOpen,
    handleTriggerClick,
    handleTriggerKeyDown,
    handleFallbackListboxClick,
  } = useSelectFallbackController({
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
  });

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return;
    mountedRef.current = true;
    const handleChange = (event: Event) => {
      const detail = (event as CustomEvent<{ value?: unknown }>).detail;
      const nextValue =
        typeof detail?.value === 'string' ? detail.value : element.value;
      if (typeof nextValue !== 'string' || nextValue === valueRef.current)
        return;
      element.dataset.valueEmpty = nextValue === '' ? 'true' : 'false';
      onChangeRef.current?.({
        currentTarget: { value: nextValue },
        target: { value: nextValue },
      });
    };
    element.addEventListener('change', handleChange);
    const handleFocusOut = (event: FocusEvent) => {
      const relatedTarget = event.relatedTarget;
      if (relatedTarget instanceof Node && element.contains(relatedTarget))
        return;
      if (hasSelectRuntime(element)) {
        element.close?.(false);
        fallbackOpenRef.current = false;
      } else {
        setFallbackOpen(false);
      }
    };
    element.addEventListener('focusout', handleFocusOut);
    void ensureRuntime();
    return () => {
      mountedRef.current = false;
      runtimeStatusRef.current = 'idle';
      fallbackOpenRef.current = false;
      element.removeEventListener('change', handleChange);
      element.removeEventListener('focusout', handleFocusOut);
      element._destroy?.();
    };
  }, [ensureRuntime, setFallbackOpen]);

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return;
    if (disabledState) {
      // Clear the fallback latch even when the runtime attaches in the same
      // commit. Otherwise the late runtime promise could reopen a disabled
      // control after this effect has already closed it.
      fallbackOpenRef.current = false;
      if (hasSelectRuntime(element)) element.close?.(false);
      else setFallbackOpen(false);
    }
    if (!element.refresh) return;
    element.refresh();
    if (element.value !== effectiveValue) element.value = effectiveValue;
    element.dataset.valueEmpty = effectiveValue === '' ? 'true' : 'false';
  }, [disabledState, effectiveValue, optionsSignature, setFallbackOpen]);

  const triggerAria: AriaAttributes = {
    'aria-describedby': ariaDescribedBy,
    'aria-disabled': disabledState || undefined,
    'aria-invalid': ariaInvalid,
    'aria-label': ariaLabel,
    'aria-labelledby': ariaLabelledBy,
    'aria-autocomplete': 'none',
    'aria-required': required || undefined,
  };

  const selectedOption =
    hasPlaceholder && selected?.value === '' ? undefined : selected;

  return (
    <div
      {...rootAttributes}
      className={['select', 'a3s-flow-select-control', className]
        .filter(Boolean)
        .join(' ')}
      data-a3s-components="select"
      data-a3s-select="flow"
      data-disabled={disabledState || undefined}
      data-placeholder={hasPlaceholder ? placeholder || '' : undefined}
      data-value-empty={effectiveValue === '' ? 'true' : 'false'}
      id={id}
      ref={(element) => {
        elementRef.current = element as SelectElement | null;
      }}
    >
      <button
        {...triggerAttributes}
        {...triggerAria}
        aria-controls={`${id}-listbox`}
        aria-expanded="false"
        aria-haspopup="listbox"
        disabled={disabledState}
        id={triggerId}
        onBlur={onBlur}
        onClick={handleTriggerClick}
        onFocus={onFocus}
        onKeyDown={handleTriggerKeyDown}
        role="combobox"
        tabIndex={tabIndex}
        title={title}
        type="button"
      >
        <span>{selectedLabel}</span>
        <DesignerIcon name="chevron-down" size={14} />
      </button>
      <div aria-hidden="true" data-popover id={`${id}-popover`}>
        <div
          aria-labelledby={triggerId}
          aria-orientation="vertical"
          id={`${id}-listbox`}
          onPointerDown={(event) => {
            // The A3S UI runtime closes a select when its trigger loses focus.
            // A pointer press on an option would otherwise move focus away
            // before the runtime's delegated click handler can publish the
            // new value, leaving the pointer-up on the underlying panel.
            // Keep focus on the trigger until the option click is processed.
            const target = event.target;
            if (
              target instanceof Element &&
              target.closest('[role="option"]')
            ) {
              event.preventDefault();
            }
          }}
          onClick={handleFallbackListboxClick}
          role="listbox"
        >
          {options.map((option, index) => (
            <div
              aria-disabled={option.disabled || undefined}
              aria-selected={option === selectedOption || undefined}
              data-label={option.label}
              data-value={option.value}
              id={`${id}-option-${index + 1}`}
              key={`${option.label}-${option.value}-${index}`}
              role="option"
            >
              {option.label}
            </div>
          ))}
        </div>
      </div>
      <input
        className="a3s-form-visually-hidden"
        disabled={disabledState}
        id={`${id}-value`}
        name={name ?? `${id}-value`}
        readOnly
        type="hidden"
        value={effectiveValue}
      />
    </div>
  );
}
