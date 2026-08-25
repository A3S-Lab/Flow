import {
  Children,
  isValidElement,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  type AriaAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
} from 'react';
import { loadA3SUIRuntime } from './a3s-ui-runtime';
import { DesignerIcon } from './designer-icons';

type SelectElement = HTMLElement & {
  _destroy?: () => void;
  refresh?: () => void;
  value?: string;
};

function loadSelectRuntime(): Promise<void> {
  return loadA3SUIRuntime('select', () => import('@a3s-lab/ui/select'));
}

export type SelectControlChangeEvent = {
  currentTarget: { value: string };
  target: { value: string };
};

export interface SelectControlProps extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  | 'children'
  | 'defaultValue'
  | 'multiple'
  | 'onBlur'
  | 'onChange'
  | 'onFocus'
  | 'value'
> {
  children: ReactNode;
  onBlur?: () => void;
  onChange?: (event: SelectControlChangeEvent) => void;
  onFocus?: () => void;
  value?: string;
}

type SelectOption = {
  disabled: boolean;
  label: string;
  value: string;
};

function selectOptions(children: ReactNode): SelectOption[] {
  return Children.toArray(children).flatMap((child) => {
    if (
      !isValidElement<{
        children?: ReactNode;
        disabled?: boolean;
        value?: unknown;
      }>(child)
    ) {
      return [];
    }
    if (child.type !== 'option') return [];
    const value = String(child.props.value ?? '');
    const label = Children.toArray(child.props.children).join('');
    return [{ disabled: Boolean(child.props.disabled), label, value }];
  });
}

/**
 * Controlled A3S UI Select adapter used by Flow composite widgets.
 * It preserves the familiar `event.target.value` callback without falling
 * back to the browser-native select surface.
 */
export function SelectControl({
  'aria-describedby': ariaDescribedBy,
  'aria-invalid': ariaInvalid,
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  children,
  className,
  disabled = false,
  id: suppliedId,
  name,
  onBlur,
  onChange,
  onFocus,
  required,
  value = '',
}: SelectControlProps) {
  const generatedId = useId();
  const id = suppliedId ?? `a3s-flow-select-${generatedId.replaceAll(':', '')}`;
  const options = useMemo(() => selectOptions(children), [children]);
  const selected =
    options.find((option) => option.value === value) ?? options[0];
  const elementRef = useRef<SelectElement | null>(null);
  const valueRef = useRef(value);
  const onChangeRef = useRef(onChange);
  valueRef.current = value;
  onChangeRef.current = onChange;

  const synchronize = useCallback((element: HTMLElement) => {
    const select = element as SelectElement;
    select.refresh?.();
    if (select.value !== valueRef.current) select.value = valueRef.current;
  }, []);

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return;
    let active = true;
    const handleChange = (event: Event) => {
      const detail = (event as CustomEvent<{ value?: unknown }>).detail;
      const nextValue =
        typeof detail?.value === 'string' ? detail.value : element.value;
      if (typeof nextValue !== 'string' || nextValue === valueRef.current)
        return;
      onChangeRef.current?.({
        currentTarget: { value: nextValue },
        target: { value: nextValue },
      });
    };
    element.addEventListener('change', handleChange);
    void loadSelectRuntime()
      .then(() => {
        if (active) synchronize(element);
      })
      .catch((error: unknown) => {
        if (!active || typeof window === 'undefined') return;
        console.error(
          '[A3S Flow] Failed to initialize the select control.',
          error,
        );
      });
    return () => {
      active = false;
      element.removeEventListener('change', handleChange);
      element._destroy?.();
    };
  }, [synchronize]);

  useEffect(() => {
    const element = elementRef.current;
    if (!element) return;
    element.refresh?.();
    if (element.value !== value) element.value = value;
  }, [options, value]);

  const triggerAria: AriaAttributes = {
    'aria-describedby': ariaDescribedBy,
    'aria-invalid': ariaInvalid,
    'aria-label': ariaLabel,
    'aria-labelledby': ariaLabelledBy,
    'aria-required': required || undefined,
  };

  return (
    <div
      className={['select', 'a3s-flow-select-control', className]
        .filter(Boolean)
        .join(' ')}
      data-disabled={disabled || undefined}
      data-placeholder=""
      id={id}
      ref={(element) => {
        elementRef.current = element as SelectElement | null;
      }}
    >
      <button
        {...triggerAria}
        aria-controls={`${id}-listbox`}
        aria-expanded="false"
        aria-haspopup="listbox"
        disabled={disabled}
        id={`${id}-trigger`}
        onBlur={onBlur}
        onFocus={onFocus}
        role="combobox"
        type="button"
      >
        <span>{selected?.label ?? ''}</span>
        <DesignerIcon name="chevron-down" size={14} />
      </button>
      <div aria-hidden="true" data-popover id={`${id}-popover`}>
        <div
          aria-labelledby={`${id}-trigger`}
          aria-orientation="vertical"
          id={`${id}-listbox`}
          role="listbox"
        >
          {options.map((option, index) => (
            <div
              aria-disabled={option.disabled || undefined}
              aria-selected={option.value === value || undefined}
              data-label={option.label}
              data-value={option.value}
              id={`${id}-option-${index + 1}`}
              key={option.value}
              role="option"
            >
              {option.label}
            </div>
          ))}
        </div>
      </div>
      <input
        className="a3s-form-visually-hidden"
        name={name ?? id}
        readOnly
        type="hidden"
        value={value}
      />
    </div>
  );
}
