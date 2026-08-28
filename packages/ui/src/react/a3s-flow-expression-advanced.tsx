import { analyzeExpression, type FormExpression } from '@a3s-lab/ui/form/core';
import { useEffect, useRef, useState, type ChangeEvent } from 'react';
import {
  A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES,
  VariableSuggestionList,
  useVariableSuggestionState,
  type A3SFlowExpressionVariable,
} from './a3s-flow-variable-picker';
import { WorkflowCodeEditor } from './workflow-code-editor';

function isChinese(locale: string): boolean {
  return locale.toLocaleLowerCase().startsWith('zh');
}

function isInsideJsonString(source: string, position: number): boolean {
  let quoted = false;
  let escaped = false;
  for (let index = 0; index < position; index += 1) {
    const character = source[index];
    if (escaped) {
      escaped = false;
      continue;
    }
    if (character === '\\') {
      escaped = true;
      continue;
    }
    if (character === '"') quoted = !quoted;
  }
  return quoted;
}

function variableMarker(source: string, caret: number): number | undefined {
  const marker = source.lastIndexOf('$', Math.max(0, caret - 1));
  if (marker < 0) return undefined;
  const query = source.slice(marker + 1, caret);
  return /^[\w.-]*$/u.test(query) ? marker : undefined;
}

export function AdvancedExpressionEditor({
  id,
  expression,
  onChange,
  locale,
  disabled,
  invalid,
  describedBy,
  draftSource,
  draftInvalid,
  onInvalidDraft,
  variables = A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES,
}: {
  id: string;
  expression: FormExpression;
  onChange: (expression: FormExpression) => void;
  locale: string;
  disabled?: boolean;
  invalid?: boolean;
  describedBy?: string;
  draftSource?: string;
  draftInvalid?: boolean;
  onInvalidDraft: (source: string) => void;
  variables?: readonly A3SFlowExpressionVariable[];
}) {
  const chinese = isChinese(locale);
  const serializedExpression = JSON.stringify(expression, null, 2);
  const externalSource = draftSource ?? serializedExpression;
  const [draft, setDraftState] = useState(externalSource);
  const [parseError, setParseError] = useState(Boolean(draftInvalid));
  const errorId = `${id}-draft-error`;
  const suggestionsId = `${id}-variables`;
  const editorRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const invocationRef = useRef<number | undefined>(undefined);
  const lastExternalSourceRef = useRef(externalSource);
  const localEditRef = useRef(false);
  const suggestions = useVariableSuggestionState(variables);

  useEffect(() => {
    if (externalSource !== lastExternalSourceRef.current) {
      lastExternalSourceRef.current = externalSource;
      if (localEditRef.current) {
        // Preserve the exact source produced by the editor when FormRenderer
        // echoes the parsed expression back through controlled props.
        localEditRef.current = false;
      } else {
        setDraftState(externalSource);
      }
    }
    setParseError(Boolean(draftInvalid));
  }, [draftInvalid, externalSource]);

  const setDraft = (next: string) => {
    localEditRef.current = true;
    setDraftState(next);
  };

  useEffect(() => {
    if (!suggestions.open) return;
    const handlePointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && !editorRef.current?.contains(event.target)) {
        invocationRef.current = undefined;
        suggestions.close();
      }
    };
    document.addEventListener('pointerdown', handlePointerDown, true);
    return () => document.removeEventListener('pointerdown', handlePointerDown, true);
  }, [suggestions]);

  const updateDraft = (next: string) => {
    setDraft(next);
    try {
      const parsed = JSON.parse(next) as unknown;
      analyzeExpression(parsed);
      setParseError(false);
      onChange(parsed as FormExpression);
    } catch {
      setParseError(true);
      onInvalidDraft(next);
    }
  };

  const detectSuggestions = (next: string, event: ChangeEvent<HTMLTextAreaElement>) => {
    const caret = event.target.selectionStart ?? next.length;
    const marker = variableMarker(next, caret);
    invocationRef.current = marker;
    if (marker === undefined) {
      suggestions.close();
      return;
    }
    suggestions.setQuery(next.slice(marker + 1, caret));
    suggestions.setOpen(true);
  };

  const chooseVariable = (variable: A3SFlowExpressionVariable) => {
    const textarea = textareaRef.current;
    const selectionStart = textarea?.selectionStart ?? draft.length;
    const selectionEnd = textarea?.selectionEnd ?? selectionStart;
    const marker = invocationRef.current;
    const invokedBySigil = marker !== undefined;
    let start = marker ?? selectionStart;
    let end = selectionEnd;
    let replacement: string;

    if (invokedBySigil && isInsideJsonString(draft, start)) {
      replacement = variable.path;
    } else {
      replacement = JSON.stringify({ op: 'field', path: variable.path }, null, invokedBySigil ? 0 : 2);
      if (!invokedBySigil && selectionStart === selectionEnd) {
        start = 0;
        end = draft.length;
      }
    }

    const next = `${draft.slice(0, start)}${replacement}${draft.slice(end)}`;
    invocationRef.current = undefined;
    suggestions.close();
    updateDraft(next);
    requestAnimationFrame(() => {
      const caret = start + replacement.length;
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(caret, caret);
    });
  };

  return (
    <div className="a3s-form-flow-expression-advanced" data-invalid={parseError || undefined} ref={editorRef}>
      <WorkflowCodeEditor
        ariaControls={suggestions.open ? suggestionsId : undefined}
        ariaExpanded={suggestions.open}
        ariaHasPopup="listbox"
        ariaLabel={chinese ? '高级表达式 JSON' : 'Advanced expression JSON'}
        className="a3s-form-flow-expression-code-editor"
        describedBy={
          [describedBy, parseError ? errorId : undefined]
            .filter(Boolean)
            .join(' ') || undefined
        }
        disabled={disabled}
        fileName={chinese ? '表达式 JSON' : 'Expression JSON'}
        id={id}
        invalid={Boolean(invalid || parseError)}
        language="json"
        locale={locale}
        onChange={(next, event) => {
          updateDraft(next);
          detectSuggestions(next, event);
        }}
        onKeyDown={(event) => {
          if (suggestions.handleKeyDown(event, chooseVariable)) return;
        }}
        status={
          parseError
            ? chinese
              ? '表达式无效'
              : 'Invalid expression'
            : chinese
              ? '表达式有效'
              : 'Valid expression'
        }
        textareaRef={textareaRef}
        toolbar={
          <button
            aria-expanded={suggestions.open}
            aria-label={chinese ? '插入变量' : 'Insert variable'}
            disabled={disabled}
            onClick={() => {
              invocationRef.current = undefined;
              suggestions.setQuery('');
              suggestions.setOpen((current) => !current);
            }}
            title={chinese ? '插入变量（也可直接输入 $）' : 'Insert variable (or type $)'}
            type="button"
          >
            <span aria-hidden="true">$</span>
            {chinese ? '变量' : 'Variable'}
          </button>
        }
        value={draft}
      />
      {suggestions.open && (
        <VariableSuggestionList
          activeIndex={suggestions.activeIndex}
          id={suggestionsId}
          locale={locale}
          onActiveIndexChange={suggestions.setActiveIndex}
          onSelect={chooseVariable}
          query={suggestions.query}
          variables={variables}
        />
      )}
      <small id={errorId} role={parseError ? 'alert' : undefined}>
        {parseError
          ? chinese
            ? '表达式 JSON 无效，请检查括号、引号和字段名。'
            : 'Invalid expression JSON. Check brackets, quotes, and field names.'
          : chinese
            ? '输入 $ 或点击“变量”可引用工作流输入、全局变量和上游输出。'
            : 'Type $ or choose Variable to reference inputs, globals, and upstream outputs.'}
      </small>
    </div>
  );
}
