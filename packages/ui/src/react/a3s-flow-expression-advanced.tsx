import { analyzeExpression, type FormExpression } from '@a3s-lab/ui/form/core';
import { useEffect, useMemo, useRef, useState, type ChangeEvent } from 'react';
import { DesignerIcon } from './designer-icons';
import {
  A3S_FLOW_DEFAULT_EXPRESSION_VARIABLES,
  VariableSuggestionList,
  useVariableSuggestionState,
  type A3SFlowExpressionVariable,
} from './a3s-flow-variable-picker';

let codeEditorRuntimePromise: Promise<void> | undefined;

function loadCodeEditorRuntime(): Promise<void> {
  if (typeof window === 'undefined') return Promise.resolve();
  codeEditorRuntimePromise ??= (async () => {
    if (!window.basecoat) await import('@a3s-lab/ui/basecoat');
    await import('@a3s-lab/ui/code-editor');
    window.basecoat?.init('code-editor');
    window.basecoat?.start();
  })();
  return codeEditorRuntimePromise;
}

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
  const source = draftSource ?? JSON.stringify(expression, null, 2);
  const [draft, setDraft] = useState(source);
  const [parseError, setParseError] = useState(Boolean(draftInvalid));
  const errorId = `${id}-draft-error`;
  const suggestionsId = `${id}-variables`;
  const editorRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const invocationRef = useRef<number | undefined>(undefined);
  const suggestions = useVariableSuggestionState(variables);
  const lines = useMemo(() => draft.split('\n'), [draft]);

  useEffect(() => {
    let active = true;
    void loadCodeEditorRuntime().then(() => {
      if (active && editorRef.current) {
        window.basecoat?.refresh(editorRef.current);
      }
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    setDraft(source);
    setParseError(Boolean(draftInvalid));
  }, [draftInvalid, source]);

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
      <div
        aria-label={chinese ? '高级表达式 JSON' : 'Advanced expression JSON'}
        className="code-editor a3s-form-flow-expression-code-editor"
        data-dirty="false"
        data-disabled={disabled ? 'true' : 'false'}
        data-language="json"
        data-line-numbers="true"
        data-size="sm"
        data-validation-state={parseError ? 'invalid' : 'valid'}
        data-wrap="false"
      >
        <header>
          <div data-code-editor-file>
            <DesignerIcon name="components" size={13} />
            <strong>{chinese ? '表达式 JSON' : 'Expression JSON'}</strong>
          </div>
          <div data-code-editor-actions>
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
          </div>
        </header>
        <section>
          <div aria-hidden="true" data-code-editor-gutter>
            {lines.map((_, index) => (
              <span data-line={index + 1} key={index}>
                {index + 1}
              </span>
            ))}
          </div>
          <textarea
            aria-controls={suggestions.open ? suggestionsId : undefined}
            aria-describedby={[describedBy, parseError ? errorId : undefined].filter(Boolean).join(' ') || undefined}
            aria-expanded={suggestions.open}
            aria-haspopup="listbox"
            aria-invalid={invalid || parseError || undefined}
            aria-label={chinese ? '高级表达式 JSON' : 'Advanced expression JSON'}
            disabled={disabled}
            id={id}
            onChange={(event) => {
              const next = event.target.value;
              updateDraft(next);
              detectSuggestions(next, event);
            }}
            onKeyDown={(event) => {
              if (suggestions.handleKeyDown(event, chooseVariable)) return;
            }}
            ref={textareaRef}
            spellCheck={false}
            value={draft}
            wrap="off"
          />
        </section>
        <footer>
          <div data-code-editor-info>
            <span data-code-editor-state>
              {parseError
                ? chinese
                  ? '表达式无效'
                  : 'Invalid expression'
                : chinese
                  ? '表达式有效'
                  : 'Valid expression'}
            </span>
            <span data-code-editor-lines>{chinese ? `${lines.length} 行` : `${lines.length} lines`}</span>
          </div>
          <div data-code-editor-meta>
            <span>JSON</span>
          </div>
        </footer>
      </div>
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
