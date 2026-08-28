import {
  useCallback,
  useEffect,
  useRef,
  type ChangeEvent,
  type FocusEventHandler,
  type KeyboardEventHandler,
  type ReactNode,
  type RefObject,
} from 'react';
import { CodeEditor } from '@a3s-lab/ui/react';
import { DesignerIcon } from './designer-icons';

type CodeEditorElement = HTMLElement & {
  refresh?: () => void;
};

function lineLabel(count: number, locale: string | undefined): string {
  const chinese = locale?.toLocaleLowerCase().startsWith('zh') === true;
  if (chinese) return `${count} 行`;
  return `${count} ${count === 1 ? 'line' : 'lines'}`;
}

function characterLabel(count: number, locale: string | undefined): string {
  const chinese = locale?.toLocaleLowerCase().startsWith('zh') === true;
  if (chinese) return `${count} 个字符`;
  return `${count} ${count === 1 ? 'character' : 'characters'}`;
}

function editorLabels(locale: string | undefined) {
  const chinese = locale?.toLocaleLowerCase().startsWith('zh') === true;
  return chinese
    ? {
        character: '个字符',
        characters: '个字符',
        dirty: '有未保存修改',
        disabled: '已禁用',
        empty: '请输入 JSON',
        invalidPrefix: 'JSON 无效，位置',
        line: '行',
        lines: '行',
        position: '第 {line} 行，第 {column} 列',
        readonly: '只读',
        saved: '已保存',
        valid: 'JSON 有效',
      }
    : {
        character: 'character',
        characters: 'characters',
        dirty: 'Unsaved changes',
        disabled: 'Disabled',
        empty: 'Enter JSON',
        invalidPrefix: 'Invalid JSON near',
        line: 'line',
        lines: 'lines',
        position: 'Ln {line}, Col {column}',
        readonly: 'Read only',
        saved: 'Saved',
        valid: 'Valid JSON',
      };
}

export interface WorkflowCodeEditorProps {
  ariaControls?: string;
  ariaExpanded?: boolean;
  ariaHasPopup?: 'listbox';
  ariaLabel: string;
  className?: string;
  describedBy?: string;
  dirty?: boolean;
  disabled?: boolean;
  fileName: string;
  id: string;
  invalid?: boolean;
  language: string;
  locale?: string;
  meta?: ReactNode;
  onBlur?: FocusEventHandler<HTMLTextAreaElement>;
  onChange?: (value: string, event: ChangeEvent<HTMLTextAreaElement>) => void;
  onFocus?: FocusEventHandler<HTMLTextAreaElement>;
  onKeyDown?: KeyboardEventHandler<HTMLTextAreaElement>;
  placeholder?: string;
  readOnly?: boolean;
  size?: 'sm' | 'lg';
  status?: ReactNode;
  textareaRef?: RefObject<HTMLTextAreaElement | null>;
  toolbar?: ReactNode;
  value: string;
  wrap?: boolean;
}

/** Theme-aware A3S UI code-editor surface shared by workflow configuration fields. */
export function WorkflowCodeEditor({
  ariaControls,
  ariaExpanded,
  ariaHasPopup,
  ariaLabel,
  className,
  describedBy,
  dirty = false,
  disabled = false,
  fileName,
  id,
  invalid = false,
  language,
  locale,
  meta,
  onBlur,
  onChange,
  onFocus,
  onKeyDown,
  placeholder,
  readOnly = false,
  size = 'sm',
  status,
  textareaRef,
  toolbar,
  value,
  wrap = false,
}: WorkflowCodeEditorProps) {
  const editorRef = useRef<CodeEditorElement | null>(null);
  const labels = editorLabels(locale);

  const synchronize = useCallback(() => {
    const editor = editorRef.current;
    if (!editor) return;
    editor.refresh?.();
    const textarea = editor.querySelector<HTMLTextAreaElement>(
      ':scope > section > textarea',
    );
    if (!textarea) return;
    // The shared runtime owns syntax validation. Flow can still report a
    // domain-level error for valid JSON, so keep that state independent from
    // the runtime's data-validation-state attribute.
    if (invalid) textarea.setAttribute('aria-invalid', 'true');
  }, [invalid]);

  useEffect(() => {
    synchronize();
  }, [disabled, dirty, language, readOnly, synchronize, value]);

  return (
    <CodeEditor
      aria-label={ariaLabel}
      className={['a3s-form-workflow-code-editor', className]
        .filter(Boolean)
        .join(' ')}
      data-dirty={dirty ? 'true' : 'false'}
      data-disabled={disabled ? 'true' : 'false'}
      data-flow-invalid={invalid ? 'true' : 'false'}
      data-indent-size="2"
      data-language={language}
      data-label-character={labels.character}
      data-label-characters={labels.characters}
      data-label-dirty={labels.dirty}
      data-label-disabled={labels.disabled}
      data-label-empty={labels.empty}
      data-label-invalid-prefix={labels.invalidPrefix}
      data-label-line={labels.line}
      data-label-lines={labels.lines}
      data-label-position={labels.position}
      data-label-readonly={labels.readonly}
      data-label-saved={labels.saved}
      data-label-valid={labels.valid}
      data-line-numbers="true"
      data-size={size}
      data-validation={language.toLocaleLowerCase() === 'json' ? 'json' : undefined}
      data-wrap={wrap ? 'true' : 'false'}
      onReady={(element) => {
        editorRef.current = element as CodeEditorElement;
        synchronize();
      }}
      ref={(element) => {
        editorRef.current = element as CodeEditorElement | null;
      }}
    >
      <header>
        <div data-code-editor-file>
          <DesignerIcon name="file" size={13} />
          <strong>{fileName}</strong>
        </div>
        <div data-code-editor-actions>
          <span data-code-editor-language>{language.toLocaleUpperCase()}</span>
          {toolbar}
        </div>
      </header>
      <section>
        <div aria-hidden="true" data-code-editor-gutter role="presentation" />
        <textarea
          aria-controls={ariaControls}
          aria-describedby={describedBy}
          aria-expanded={ariaExpanded}
          aria-haspopup={ariaHasPopup}
          aria-invalid={invalid || undefined}
          aria-label={ariaLabel}
          disabled={disabled}
          id={id}
          onBlur={onBlur}
          onChange={
            onChange
              ? (event) => onChange(event.target.value, event)
              : undefined
          }
          onFocus={onFocus}
          onKeyDown={onKeyDown}
          placeholder={placeholder}
          readOnly={readOnly}
          ref={textareaRef}
          spellCheck={false}
          value={value}
          wrap={wrap ? 'soft' : 'off'}
        />
      </section>
      <footer>
        <div data-code-editor-info>
          <span data-code-editor-state aria-live="polite">
            {status ?? labels.saved}
          </span>
          <span data-code-editor-lines>{lineLabel(value.split('\n').length, locale)}</span>
          <span data-code-editor-characters>
            {characterLabel(
              Array.from(value).filter(
                (character) => character !== '\n' && character !== '\r',
              ).length,
              locale,
            )}
          </span>
        </div>
        <div data-code-editor-meta>
          <output data-code-editor-message />
          <output
            aria-label={locale?.toLocaleLowerCase().startsWith('zh') ? '光标位置' : 'Cursor position'}
            data-code-editor-position
          />
          {meta ?? <span>{language.toLocaleUpperCase()}</span>}
        </div>
      </footer>
    </CodeEditor>
  );
}
