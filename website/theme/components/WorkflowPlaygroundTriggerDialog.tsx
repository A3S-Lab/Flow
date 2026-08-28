import { BracketsCurly, Lightning, X } from '@phosphor-icons/react';
import {
  useEffect,
  useId,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent,
  type ReactNode,
} from 'react';
import type { WorkflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  cleanTriggerValue,
  createTriggerDraft,
  isTriggerObject,
  triggerSchemaProperties,
  triggerSchemaRequired,
  triggerValueAtPath,
  setTriggerValueAtPath,
  triggerSchemaAllowsType,
  triggerSchemaType,
  validateTriggerInput,
  stableTriggerJson,
  type PlaygroundTriggerFieldError,
  type PlaygroundTriggerSchema,
  type PlaygroundTriggerValue,
} from './WorkflowPlayground.trigger';
import type { FlowWebsiteLocale } from './flow-node-catalog';
import { SelectControl } from '@a3s-lab/flow-ui/react';

type TriggerDialogProps = {
  copy: WorkflowPlaygroundCopy;
  locale: FlowWebsiteLocale;
  schema: PlaygroundTriggerSchema;
  workflowName: string;
  onClose: () => void;
  onSubmit: (value: PlaygroundTriggerValue) => void;
};

type SchemaFieldProps = {
  schema: PlaygroundTriggerSchema;
  path: string[];
  name: string;
  required: boolean;
  value: PlaygroundTriggerValue;
  locale: FlowWebsiteLocale;
  errors: ReadonlyMap<string, PlaygroundTriggerFieldError[]>;
  onChange: (path: readonly string[], value: PlaygroundTriggerValue) => void;
  onJsonError: (path: string, message?: string) => void;
};

function fieldLabel(
  schema: PlaygroundTriggerSchema,
  name: string,
  locale: FlowWebsiteLocale,
): string {
  return typeof schema.title === 'string' && schema.title.trim()
    ? schema.title
    : name || (locale === 'zh' ? '输入' : 'Input');
}

function schemaTypeLabel(
  schema: PlaygroundTriggerSchema,
  locale: FlowWebsiteLocale,
): string {
  const type = triggerSchemaType(schema);
  if (locale === 'en') return type;
  return (
    (
      {
        string: '文本',
        number: '数字',
        integer: '整数',
        boolean: '布尔值',
        object: '对象',
        array: '数组',
        null: '空值',
        value: '值',
      } as Record<string, string>
    )[type] ?? '值'
  );
}

function errorFor(
  errors: ReadonlyMap<string, PlaygroundTriggerFieldError[]>,
  path: readonly string[],
): PlaygroundTriggerFieldError[] {
  return errors.get(path.join('.')) ?? [];
}

function scalarValue(
  schema: PlaygroundTriggerSchema,
  value: PlaygroundTriggerValue | undefined,
): PlaygroundTriggerValue {
  if (value !== undefined) {
    // Nullable scalar fields use an empty control value for `null`; showing
    // the literal string "null" would make the editor look pre-filled.
    if (value === null && triggerSchemaType(schema) !== 'null') return '';
    return value;
  }
  if (triggerSchemaType(schema) === 'boolean') return false;
  return '';
}

const ENUM_VALUE_PREFIX = 'a3s-json:';

function enumOptionValue(value: PlaygroundTriggerValue): string {
  return `${ENUM_VALUE_PREFIX}${stableTriggerJson(value)}`;
}

function parseEnumOption(raw: string): PlaygroundTriggerValue {
  const encoded = raw.startsWith(ENUM_VALUE_PREFIX)
    ? raw.slice(ENUM_VALUE_PREFIX.length)
    : raw;
  try {
    return JSON.parse(encoded) as PlaygroundTriggerValue;
  } catch {
    return encoded;
  }
}

function formatJson(value: PlaygroundTriggerValue | undefined): string {
  if (value === undefined) return '';
  return JSON.stringify(value, null, 2);
}

function focusTriggerField(root: HTMLElement, path: string): void {
  const targetPath = path || 'root';
  const field = [
    ...root.querySelectorAll<HTMLElement>('[data-trigger-path]'),
  ].find((candidate) => (candidate.dataset.triggerPath ?? '') === targetPath);
  field
    ?.querySelector<HTMLElement>(
      '[role="combobox"], input:not([type="hidden"]), textarea, button',
    )
    ?.focus();
}

function SchemaField({
  schema,
  path,
  name,
  required,
  value,
  locale,
  errors,
  onChange,
  onJsonError,
}: SchemaFieldProps): ReactNode {
  const label = fieldLabel(schema, name, locale);
  const pathKey = path.join('.');
  const inputId = `trigger-field-${pathKey || 'root'}`.replace(
    /[^a-zA-Z0-9_-]/gu,
    '-',
  );
  const fieldErrors = errorFor(errors, path);
  const describedBy = fieldErrors.length ? `${inputId}-error` : undefined;
  const current = triggerValueAtPath(value, path);
  const type = triggerSchemaType(schema);
  const enumValues = schema.enum ?? [];
  const enumSelect = enumValues.length > 0;

  if (type === 'object' && !enumSelect) {
    const properties = triggerSchemaProperties(schema);
    return (
      <fieldset
        className="a3s-trigger-fieldset"
        data-trigger-path={pathKey || 'root'}
      >
        {path.length > 0 && (
          <legend>
            <span>{label}</span>
            {required && <em>{locale === 'zh' ? '必填' : 'Required'}</em>}
          </legend>
        )}
        {schema.description && (
          <p className="a3s-trigger-field__description">{schema.description}</p>
        )}
        <div className="a3s-trigger-fieldset__fields">
          {Object.entries(properties).map(([childName, childSchema]) => (
            <SchemaField
              errors={errors}
              key={`${pathKey}.${childName}`}
              locale={locale}
              name={childName}
              onChange={onChange}
              onJsonError={onJsonError}
              path={[...path, childName]}
              required={triggerSchemaRequired(schema).has(childName)}
              schema={childSchema}
              value={value}
            />
          ))}
          {Object.keys(properties).length === 0 && (
            <JsonField
              errors={fieldErrors}
              id={inputId}
              label={label}
              locale={locale}
              onChange={(next) => onChange(path, next)}
              onJsonError={onJsonError}
              path={pathKey}
              required={required}
              value={current}
              emptyValue={{}}
            />
          )}
        </div>
      </fieldset>
    );
  }

  if (type === 'array' && !enumSelect) {
    return (
      <JsonField
        errors={fieldErrors}
        id={inputId}
        label={label}
        locale={locale}
        onChange={(next) => onChange(path, next)}
        onJsonError={onJsonError}
        path={pathKey}
        required={required}
        value={current}
        description={schema.description}
        placeholder={schema.items ? '[ ]' : undefined}
      />
    );
  }

  const valueForInput = scalarValue(schema, current);
  const enumValueMatchesCurrent = enumValues.some(
    (option) =>
      current !== undefined &&
      stableTriggerJson(option) === stableTriggerJson(current),
  );
  const common = {
    'aria-describedby': describedBy,
    'aria-invalid': fieldErrors.length > 0 || undefined,
    id: inputId,
    name: pathKey,
  };
  const onScalarChange = (event: ChangeEvent<HTMLInputElement>) => {
    if (type === 'boolean') {
      onChange(path, event.target.checked);
      return;
    }
    if (type === 'number' || type === 'integer') {
      const raw = event.target.value;
      onChange(
        path,
        raw === ''
          ? triggerSchemaAllowsType(schema, 'null')
            ? null
            : ''
          : Number(raw),
      );
      return;
    }
    const raw = event.target.value;
    onChange(
      path,
      raw === '' && triggerSchemaAllowsType(schema, 'null') ? null : raw,
    );
  };

  if ((type === 'value' || type === 'null') && !enumSelect) {
    return (
      <JsonField
        errors={fieldErrors}
        id={inputId}
        label={label}
        locale={locale}
        onChange={(next) => onChange(path, next)}
        onJsonError={onJsonError}
        path={pathKey}
        required={required}
        value={current}
        description={schema.description}
        emptyValue={
          triggerSchemaAllowsType(schema, 'null')
            ? null
            : triggerSchemaAllowsType(schema, 'string')
              ? ''
              : {}
        }
        placeholder={type === 'null' ? 'null' : '{ }'}
      />
    );
  }

  return (
    <div
      className={`a3s-trigger-field${fieldErrors.length ? ' is-invalid' : ''}`}
      data-trigger-path={pathKey}
    >
      <label htmlFor={enumSelect ? `${inputId}-trigger` : inputId}>
        <span id={`${inputId}-label`}>
          {label}
          {required && (
            <em aria-label={locale === 'zh' ? '必填' : 'required'}>*</em>
          )}
        </span>
        <small>{schemaTypeLabel(schema, locale)}</small>
      </label>
      {schema.description && (
        <p className="a3s-trigger-field__description">{schema.description}</p>
      )}
      {enumSelect ? (
        <SelectControl
          {...common}
          aria-labelledby={`${inputId}-label`}
          className="a3s-trigger-field__select"
          required={required}
          value={
            current === undefined || !enumValueMatchesCurrent
              ? ''
              : enumOptionValue(current)
          }
          onChange={(event) =>
            onChange(path, parseEnumOption(event.target.value))
          }
        >
          <option value="">
            {locale === 'zh' ? '选择一个值…' : 'Choose a value…'}
          </option>
          {enumValues.map((option, index) => (
            <option
              key={`${enumOptionValue(option)}-${index}`}
              value={enumOptionValue(option)}
            >
              {typeof option === 'string' ? option : stableTriggerJson(option)}
            </option>
          ))}
        </SelectControl>
      ) : type === 'boolean' ? (
        <label className="a3s-trigger-checkbox" htmlFor={inputId}>
          <input
            {...common}
            checked={valueForInput === true}
            onChange={onScalarChange}
            type="checkbox"
          />
          <span>
            {valueForInput === true
              ? locale === 'zh'
                ? '是'
                : 'True'
              : locale === 'zh'
                ? '否'
                : 'False'}
          </span>
        </label>
      ) : (
        <input
          {...common}
          inputMode={
            type === 'number' || type === 'integer' ? 'decimal' : undefined
          }
          max={schema.maximum}
          maxLength={schema.maxLength}
          min={schema.minimum}
          minLength={schema.minLength}
          onChange={onScalarChange}
          step={type === 'integer' ? 1 : 'any'}
          type={type === 'number' || type === 'integer' ? 'number' : 'text'}
          value={String(valueForInput)}
        />
      )}
      {fieldErrors.length > 0 && (
        <div
          className="a3s-trigger-field__errors"
          id={describedBy}
          role="alert"
        >
          {fieldErrors.map((error) => (
            <span key={error.message}>{error.message}</span>
          ))}
        </div>
      )}
    </div>
  );
}

function JsonField({
  id,
  label,
  locale,
  path,
  required,
  value,
  errors,
  description,
  placeholder,
  emptyValue = [],
  onChange,
  onJsonError,
}: {
  id: string;
  label: string;
  locale: FlowWebsiteLocale;
  path: string;
  required: boolean;
  value: PlaygroundTriggerValue | undefined;
  errors: readonly PlaygroundTriggerFieldError[];
  description?: string;
  placeholder?: string;
  emptyValue?: PlaygroundTriggerValue;
  onChange: (value: PlaygroundTriggerValue) => void;
  onJsonError: (path: string, message?: string) => void;
}) {
  const [draft, setDraft] = useState(() => formatJson(value));
  useEffect(() => setDraft(formatJson(value)), [value]);
  const errorId = `${id}-error`;
  const parse = (raw: string) => {
    setDraft(raw);
    if (!raw.trim()) {
      onJsonError(path, undefined);
      onChange(emptyValue);
      return;
    }
    try {
      const parsed = JSON.parse(raw) as PlaygroundTriggerValue;
      onJsonError(path, undefined);
      onChange(parsed);
    } catch {
      onJsonError(
        path,
        locale === 'zh' ? '请输入有效的 JSON。' : 'Enter valid JSON.',
      );
    }
  };
  return (
    <div
      className={`a3s-trigger-field${errors.length ? ' is-invalid' : ''}`}
      data-trigger-path={path || 'root'}
    >
      <label htmlFor={id}>
        <span>
          {label}
          {required && (
            <em aria-label={locale === 'zh' ? '必填' : 'required'}>*</em>
          )}
        </span>
        <small>JSON</small>
      </label>
      {description && (
        <p className="a3s-trigger-field__description">{description}</p>
      )}
      <textarea
        aria-describedby={errors.length ? errorId : undefined}
        aria-invalid={errors.length > 0 || undefined}
        id={id}
        onChange={(event) => parse(event.target.value)}
        placeholder={placeholder ?? '{ }'}
        rows={4}
        value={draft}
      />
      {errors.length > 0 && (
        <div className="a3s-trigger-field__errors" id={errorId} role="alert">
          {errors.map((error) => (
            <span key={error.message}>{error.message}</span>
          ))}
        </div>
      )}
    </div>
  );
}

export function WorkflowPlaygroundTriggerDialog({
  copy,
  locale,
  schema,
  workflowName,
  onClose,
  onSubmit,
}: TriggerDialogProps) {
  const dialogRef = useRef<HTMLElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  const titleId = useId();
  const descriptionId = useId();
  const [value, setValue] = useState<PlaygroundTriggerValue>(() =>
    createTriggerDraft(schema),
  );
  const [errors, setErrors] = useState<PlaygroundTriggerFieldError[]>([]);
  const [jsonErrors, setJsonErrors] = useState<
    Record<string, PlaygroundTriggerFieldError>
  >({});

  useEffect(() => {
    previousFocus.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    const frame = window.requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLElement>(
          '[role="combobox"], input:not([type="hidden"]), textarea, button',
        )
        ?.focus();
    });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== 'Tab' || !dialogRef.current) return;
      const focusable = [
        ...new Set(
          dialogRef.current.querySelectorAll<HTMLElement>(
            'button, input:not([type="hidden"]), textarea, [role="combobox"], [href], [tabindex]',
          ),
        ),
      ].filter(
        (element) =>
          !element.hasAttribute('disabled') &&
          element.getAttribute('aria-disabled') !== 'true' &&
          element.getAttribute('tabindex') !== '-1',
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable.at(-1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => {
      window.cancelAnimationFrame(frame);
      document.removeEventListener('keydown', onKeyDown);
      previousFocus.current?.focus();
    };
  }, [onClose]);

  const allErrors = [
    ...errors,
    ...Object.values(jsonErrors).filter(
      (error) =>
        !errors.some(
          (item) => item.path === error.path && item.message === error.message,
        ),
    ),
  ];
  const errorMap = new Map<string, PlaygroundTriggerFieldError[]>();
  for (const error of allErrors)
    errorMap.set(error.path, [...(errorMap.get(error.path) ?? []), error]);

  const updateValue = (
    path: readonly string[],
    next: PlaygroundTriggerValue,
  ) => {
    setValue((current) => setTriggerValueAtPath(current, path, next));
    if (path.length > 0) {
      const pathKey = path.join('.');
      setErrors((current) =>
        current.filter(
          (error) =>
            error.path !== pathKey && !error.path.startsWith(`${pathKey}.`),
        ),
      );
    }
  };
  const updateJsonError = (path: string, message?: string) => {
    setJsonErrors((current) => {
      const next = { ...current };
      if (message) next[path] = { path, message };
      else delete next[path];
      return next;
    });
  };
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const nextErrors = validateTriggerInput(schema, value, locale);
    setErrors(nextErrors);
    if (nextErrors.length > 0 || Object.keys(jsonErrors).length > 0) {
      const firstPath = nextErrors[0]?.path || Object.keys(jsonErrors)[0];
      if (firstPath !== undefined) {
        window.requestAnimationFrame(() => {
          if (dialogRef.current)
            focusTriggerField(dialogRef.current, firstPath);
        });
      }
      return;
    }
    onSubmit(cleanTriggerValue(value));
  };

  return (
    <div
      aria-label={copy.triggerDialogTitle}
      className="a3s-workflow-dialog-backdrop a3s-trigger-dialog-backdrop"
      data-testid="trigger-dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        aria-describedby={descriptionId}
        aria-labelledby={titleId}
        aria-modal="true"
        className="a3s-trigger-dialog"
        data-testid="trigger-dialog"
        ref={dialogRef}
        role="dialog"
      >
        <header className="a3s-trigger-dialog__header">
          <span className="a3s-trigger-dialog__icon" aria-hidden="true">
            <Lightning weight="fill" />
          </span>
          <div>
            <p>{copy.triggerDialogEyebrow}</p>
            <h2 id={titleId}>{copy.triggerDialogTitle}</h2>
          </div>
          <button aria-label={copy.close} onClick={onClose} type="button">
            <X aria-hidden="true" />
          </button>
        </header>
        <p className="a3s-trigger-dialog__intro" id={descriptionId}>
          {copy.triggerDialogDescription(workflowName)}
        </p>
        <div className="a3s-trigger-dialog__contract">
          <BracketsCurly aria-hidden="true" />
          <span>
            <strong>{schema.title || copy.triggerInputLabel}</strong>
            <small>{copy.triggerSchemaHint}</small>
          </span>
        </div>
        <form onSubmit={submit} noValidate>
          <div className="a3s-trigger-dialog__fields">
            <SchemaField
              errors={errorMap}
              locale={locale}
              name=""
              onChange={updateValue}
              onJsonError={updateJsonError}
              path={[]}
              required
              schema={schema}
              value={value}
            />
          </div>
          {allErrors.length > 0 && (
            <p className="a3s-trigger-dialog__summary" role="alert">
              {copy.triggerValidationError(allErrors.length)}
            </p>
          )}
          <footer className="a3s-trigger-dialog__footer">
            <span>{copy.localRun}</span>
            <div>
              <button
                className="a3s-trigger-dialog__cancel"
                onClick={onClose}
                type="button"
              >
                {copy.close}
              </button>
              <button className="a3s-trigger-dialog__submit" type="submit">
                <Lightning aria-hidden="true" weight="fill" />
                {copy.triggerRun}
              </button>
            </div>
          </footer>
        </form>
      </section>
    </div>
  );
}
