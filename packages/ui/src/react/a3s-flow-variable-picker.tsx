import {
  createContext,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
  type TextareaHTMLAttributes,
} from 'react';
import { DesignerIcon } from './designer-icons';

export type A3SFlowExpressionVariableGroup = 'input' | 'global' | 'upstream' | 'scope';

export interface A3SFlowExpressionVariable {
  dataType?: string;
  description?: string;
  group: A3SFlowExpressionVariableGroup;
  label: string;
  nodeId?: string;
  path: string;
}

export const A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES: readonly A3SFlowExpressionVariable[] = [
  {
    group: 'input',
    label: 'Workflow input',
    path: 'input',
    dataType: 'object',
  },
  {
    group: 'global',
    label: 'Workflow run ID',
    path: 'global.run_id',
    dataType: 'string',
  },
  {
    group: 'global',
    label: 'Workflow name',
    path: 'global.workflow_name',
    dataType: 'string',
  },
];

const A3SFlowExpressionVariablesContext = createContext<
  readonly A3SFlowExpressionVariable[] | undefined
>(undefined);

export function A3SFlowExpressionVariablesProvider({
  children,
  variables,
}: {
  children?: ReactNode;
  variables?: readonly A3SFlowExpressionVariable[];
}) {
  return (
    <A3SFlowExpressionVariablesContext.Provider value={variables}>
      {children}
    </A3SFlowExpressionVariablesContext.Provider>
  );
}

export function useA3SFlowExpressionVariables(
  variables?: readonly A3SFlowExpressionVariable[],
): readonly A3SFlowExpressionVariable[] {
  const providedVariables = useContext(A3SFlowExpressionVariablesContext);
  return variables ?? providedVariables ?? A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES;
}

function isChinese(locale: string): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function normalizedSearch(value: string): string {
  return value.trim().replace(/^\$/u, '').toLocaleLowerCase();
}

export function filterA3SFlowExpressionVariables(
  variables: readonly A3SFlowExpressionVariable[],
  query: string,
): A3SFlowExpressionVariable[] {
  const normalized = normalizedSearch(query);
  if (!normalized) return [...variables];
  const terms = normalized.split(/\s+/u).filter(Boolean);
  return variables.filter((variable) => {
    const haystack = [variable.path, variable.label, variable.description, variable.dataType, variable.nodeId]
      .filter(Boolean)
      .join(' ')
      .toLocaleLowerCase();
    return terms.every((term) => haystack.includes(term));
  });
}

function groupLabel(group: A3SFlowExpressionVariableGroup, chinese: boolean): string {
  const labels = chinese
    ? {
        global: '全局变量',
        input: '工作流输入',
        scope: '当前子流程',
        upstream: '上游节点输出',
      }
    : {
        global: 'Global variables',
        input: 'Workflow input',
        scope: 'Current scope',
        upstream: 'Upstream outputs',
      };
  return labels[group];
}

export function VariableSuggestionList({
  activeIndex,
  id,
  locale,
  onActiveIndexChange,
  onSelect,
  query,
  variables,
}: {
  activeIndex: number;
  id: string;
  locale: string;
  onActiveIndexChange: (index: number) => void;
  onSelect: (variable: A3SFlowExpressionVariable) => void;
  query: string;
  variables: readonly A3SFlowExpressionVariable[];
}) {
  const chinese = isChinese(locale);
  const filtered = useMemo(() => filterA3SFlowExpressionVariables(variables, query), [query, variables]);
  const grouped = useMemo(() => {
    const groups = new Map<A3SFlowExpressionVariableGroup, { index: number; variable: A3SFlowExpressionVariable }[]>();
    filtered.forEach((variable, index) => {
      const entries = groups.get(variable.group) ?? [];
      entries.push({ index, variable });
      groups.set(variable.group, entries);
    });
    return groups;
  }, [filtered]);

  return (
    <div
      aria-label={chinese ? '变量智能感知' : 'Variable suggestions'}
      className="a3s-flow-variable-suggestions"
      id={id}
      role="listbox"
    >
      <header>
        <span aria-hidden="true">$</span>
        <strong>{chinese ? '引用变量' : 'Reference a variable'}</strong>
        <kbd>↑↓</kbd>
        <kbd>Enter</kbd>
      </header>
      {filtered.length === 0 ? (
        <p>{chinese ? `没有匹配“${query}”的变量` : `No variables match “${query}”`}</p>
      ) : (
        <div className="a3s-flow-variable-suggestions__groups">
          {[...grouped.entries()].map(([group, entries]) => (
            <section aria-label={groupLabel(group, chinese)} key={group}>
              <h4>{groupLabel(group, chinese)}</h4>
              {entries.map(({ index, variable }) => (
                <button
                  aria-selected={index === activeIndex}
                  className={index === activeIndex ? 'is-active' : undefined}
                  data-variable-option=""
                  key={`${variable.group}-${variable.path}`}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseEnter={() => onActiveIndexChange(index)}
                  onClick={() => onSelect(variable)}
                  role="option"
                  type="button"
                >
                  <span>
                    <code>${variable.path}</code>
                    <small>{variable.label}</small>
                  </span>
                  {variable.dataType && <em>{variable.dataType}</em>}
                </button>
              ))}
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

export function useVariableSuggestionState(variables: readonly A3SFlowExpressionVariable[]) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const filtered = useMemo(() => filterA3SFlowExpressionVariables(variables, query), [query, variables]);

  useEffect(() => setActiveIndex(0), [query]);

  const close = () => {
    setOpen(false);
    setQuery('');
    setActiveIndex(0);
  };

  const handleKeyDown = (
    event: KeyboardEvent<HTMLElement>,
    onSelect: (variable: A3SFlowExpressionVariable) => void,
  ): boolean => {
    if (!open) return false;
    if (event.key === 'Escape') {
      event.preventDefault();
      close();
      return true;
    }
    if (filtered.length === 0) return false;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const direction = event.key === 'ArrowDown' ? 1 : -1;
      setActiveIndex((current) => (current + direction + filtered.length) % filtered.length);
      return true;
    }
    if (event.key === 'Enter') {
      event.preventDefault();
      onSelect(filtered[Math.min(activeIndex, filtered.length - 1)]);
      close();
      return true;
    }
    return false;
  };

  return {
    activeIndex,
    close,
    filtered,
    handleKeyDown,
    open,
    query,
    setActiveIndex,
    setOpen,
    setQuery,
  };
}

function useOutsideDismiss(ref: RefObject<HTMLElement | null>, open: boolean, onClose: () => void) {
  useEffect(() => {
    if (!open) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !ref.current?.contains(event.target)) {
        onClose();
      }
    };
    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => document.removeEventListener('pointerdown', handlePointerDown, true);
  }, [onClose, open, ref]);
}

export interface VariableReferenceInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'onChange' | 'value'> {
  locale: string;
  onPathChange: (path: string) => void;
  path: string;
  variables?: readonly A3SFlowExpressionVariable[];
}

export function VariableReferenceInput({
  locale,
  onPathChange,
  path,
  variables = A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES,
  ...props
}: VariableReferenceInputProps) {
  const generatedId = useId();
  const suggestionsId = `${props.id ?? generatedId}-variables`;
  const rootRef = useRef<HTMLDivElement>(null);
  const state = useVariableSuggestionState(variables);
  useOutsideDismiss(rootRef, state.open, state.close);

  const choose = (variable: A3SFlowExpressionVariable) => {
    onPathChange(variable.path);
    state.close();
  };

  return (
    <div className="a3s-flow-variable-input" data-open={state.open || undefined} ref={rootRef}>
      <span aria-hidden="true" className="a3s-flow-variable-input__sigil">
        $
      </span>
      <input
        {...props}
        aria-controls={state.open ? suggestionsId : undefined}
        aria-expanded={state.open}
        aria-haspopup="listbox"
        autoComplete="off"
        className={['input', props.className].filter(Boolean).join(' ')}
        value={path}
        onChange={(event) => {
          const raw = event.target.value;
          const marker = raw.lastIndexOf('$');
          const next = marker < 0 ? raw : `${raw.slice(0, marker)}${raw.slice(marker + 1)}`;
          onPathChange(next);
          if (marker >= 0) {
            state.setQuery(raw.slice(marker + 1));
            state.setOpen(true);
          } else if (state.open) {
            state.setQuery(next);
          }
        }}
        onKeyDown={(event) => {
          if (state.handleKeyDown(event, choose)) return;
          props.onKeyDown?.(event);
        }}
      />
      <button
        aria-label={isChinese(locale) ? '选择变量' : 'Choose variable'}
        className="a3s-flow-variable-input__trigger"
        disabled={props.disabled}
        onClick={() => {
          state.setQuery(path);
          state.setOpen((current) => !current);
        }}
        title={isChinese(locale) ? '输入 $ 或点击选择变量' : 'Type $ or choose a variable'}
        type="button"
      >
        <DesignerIcon name="sparkles" size={13} />
      </button>
      {state.open && (
        <VariableSuggestionList
          activeIndex={state.activeIndex}
          id={suggestionsId}
          locale={locale}
          onActiveIndexChange={state.setActiveIndex}
          onSelect={choose}
          query={state.query}
          variables={variables}
        />
      )}
    </div>
  );
}

export interface VariableTemplateTextareaProps extends Omit<
  TextareaHTMLAttributes<HTMLTextAreaElement>,
  'onChange' | 'value'
> {
  locale: string;
  onValueChange: (value: string) => void;
  value: string;
  variables?: readonly A3SFlowExpressionVariable[];
}

export function VariableTemplateTextarea({
  locale,
  onValueChange,
  value,
  variables = A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES,
  ...props
}: VariableTemplateTextareaProps) {
  const generatedId = useId();
  const suggestionsId = `${props.id ?? generatedId}-variables`;
  const rootRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const markerRef = useRef<number | undefined>(undefined);
  const state = useVariableSuggestionState(variables);
  useOutsideDismiss(rootRef, state.open, state.close);

  const choose = (variable: A3SFlowExpressionVariable) => {
    const textarea = textareaRef.current;
    const caret = textarea?.selectionStart ?? value.length;
    const end = textarea?.selectionEnd ?? caret;
    const start = markerRef.current ?? caret;
    const replacement = `{{${variable.path}}}`;
    onValueChange(`${value.slice(0, start)}${replacement}${value.slice(end)}`);
    markerRef.current = undefined;
    state.close();
    requestAnimationFrame(() => {
      const nextCaret = start + replacement.length;
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCaret, nextCaret);
    });
  };

  return (
    <div className="a3s-flow-variable-textarea" data-open={state.open || undefined} ref={rootRef}>
      <textarea
        {...props}
        aria-controls={state.open ? suggestionsId : undefined}
        aria-expanded={state.open}
        aria-haspopup="listbox"
        className={['textarea', props.className].filter(Boolean).join(' ')}
        onChange={(event) => {
          const next = event.target.value;
          onValueChange(next);
          const caret = event.target.selectionStart ?? next.length;
          const marker = next.lastIndexOf('$', Math.max(0, caret - 1));
          const query = marker >= 0 ? next.slice(marker + 1, caret) : '';
          if (marker >= 0 && /^[\w.-]*$/u.test(query)) {
            markerRef.current = marker;
            state.setQuery(query);
            state.setOpen(true);
          } else {
            markerRef.current = undefined;
            state.close();
          }
        }}
        onKeyDown={(event) => {
          if (state.handleKeyDown(event, choose)) return;
          props.onKeyDown?.(event);
        }}
        ref={textareaRef}
        value={value}
      />
      <button
        aria-label={isChinese(locale) ? '插入变量' : 'Insert variable'}
        className="a3s-flow-variable-textarea__trigger"
        disabled={props.disabled}
        onClick={() => {
          markerRef.current = undefined;
          state.setQuery('');
          state.setOpen((current) => !current);
        }}
        title={isChinese(locale) ? '输入 $ 或点击插入变量' : 'Type $ or insert a variable'}
        type="button"
      >
        <span aria-hidden="true">$</span>
        {isChinese(locale) ? '变量' : 'Variable'}
      </button>
      {state.open && (
        <VariableSuggestionList
          activeIndex={state.activeIndex}
          id={suggestionsId}
          locale={locale}
          onActiveIndexChange={state.setActiveIndex}
          onSelect={choose}
          query={state.query}
          variables={variables}
        />
      )}
    </div>
  );
}
