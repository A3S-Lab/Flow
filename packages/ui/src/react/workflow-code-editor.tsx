import {
  useEffect,
  useMemo,
  useRef,
  type ChangeEvent,
  type FocusEventHandler,
  type KeyboardEventHandler,
  type ReactNode,
  type RefObject,
} from 'react';
import { loadA3SUIRuntime } from './a3s-ui-runtime';
import { DesignerIcon } from './designer-icons';

function loadCodeEditorRuntime(): Promise<void> {
  return loadA3SUIRuntime(
    'code-editor',
    () => import('@a3s-lab/ui/code-editor'),
  );
}

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
  const editorRef = useRef<HTMLDivElement>(null);
  const lines = useMemo(() => value.split('\n'), [value]);
  const characterCount = useMemo(
    () =>
      Array.from(value).filter(
        (character) => character !== '\n' && character !== '\r',
      ).length,
    [value],
  );
  const labels = editorLabels(locale);

  useEffect(() => {
    let active = true;
    void loadCodeEditorRuntime()
      .then(() => {
        if (active && editorRef.current && typeof window !== 'undefined') {
          window.basecoat?.refresh(editorRef.current);
        }
      })
      .catch((error: unknown) => {
        if (!active || typeof window === 'undefined') return;
        console.error(
          '[A3S Flow] Failed to initialize the code editor.',
          error,
        );
      });
    return () => {
      active = false;
    };
  }, []);

  return (
    <div
      aria-label={ariaLabel}
      className={['code-editor', 'a3s-form-workflow-code-editor', className]
        .filter(Boolean)
        .join(' ')}
      data-dirty={dirty ? 'true' : 'false'}
      data-disabled={disabled ? 'true' : 'false'}
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
      data-validation-state={invalid ? 'invalid' : 'valid'}
      data-wrap={wrap ? 'true' : 'false'}
      ref={editorRef}
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
        <div
          aria-hidden="true"
          data-code-editor-gutter
          data-line-count={lines.length}
        >
          {lines.map((_, index) => (
            <span data-line={index + 1} key={index}>
              {index + 1}
            </span>
          ))}
        </div>
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
          {status && <span data-code-editor-state>{status}</span>}
          <span data-code-editor-lines>{lineLabel(lines.length, locale)}</span>
          <span data-code-editor-characters>
            {characterLabel(characterCount, locale)}
          </span>
        </div>
        <div data-code-editor-meta>
          {meta ?? <span>{language.toLocaleUpperCase()}</span>}
        </div>
      </footer>
    </div>
  );
}
