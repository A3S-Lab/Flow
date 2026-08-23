import {
  ArrowCounterClockwise,
  Check,
  Copy,
  Plus,
  Trash,
  WarningCircle,
} from '@phosphor-icons/react';
import { useLang } from '@rspress/core/runtime';
import {
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react';
import {
  initialNodeValues,
  isFieldVisible,
  localized,
  nodeCategories,
  nodeConfigCopy,
  nodeDefinitions,
  validateAndSerialize,
  type NodeCategoryId,
  type NodeConfigField,
  type NodeConfigLocale,
  type NodeDefinition,
  type NodeFormValues,
  type NodeRepeaterField,
  type NodeScalarField,
} from './NodeConfigLab.data';

type CopyState = 'copied' | 'error' | 'idle';
type ScalarValue = string | number | boolean;

export default function NodeConfigLab() {
  const locale: NodeConfigLocale = useLang() === 'en' ? 'en' : 'zh';
  const copy = nodeConfigCopy[locale];
  const categoryRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [activeCategory, setActiveCategory] = useState<NodeCategoryId>('work');
  const [activeNodeId, setActiveNodeId] = useState('schedule_step');
  const activeDefinition =
    nodeDefinitions.find(({ id }) => id === activeNodeId) ?? nodeDefinitions[0];
  const [drafts, setDrafts] = useState<Record<string, NodeFormValues>>(() => ({
    [activeDefinition.id]: initialNodeValues(activeDefinition),
  }));
  const [copyState, setCopyState] = useState<CopyState>('idle');
  const values =
    drafts[activeDefinition.id] ?? initialNodeValues(activeDefinition);
  const result = useMemo(
    () => validateAndSerialize(activeDefinition, values, locale),
    [activeDefinition, locale, values],
  );
  const visibleNodes = nodeDefinitions.filter(
    ({ category }) => category === activeCategory,
  );

  const selectNode = (definition: NodeDefinition) => {
    setActiveCategory(definition.category);
    setActiveNodeId(definition.id);
    setDrafts((current) =>
      current[definition.id]
        ? current
        : {
            ...current,
            [definition.id]: initialNodeValues(definition),
          },
    );
    setCopyState('idle');
  };

  const selectCategory = (category: NodeCategoryId) => {
    setActiveCategory(category);
    const first = nodeDefinitions.find(
      (definition) => definition.category === category,
    );
    if (first) selectNode(first);
  };

  const handleCategoryKeyDown = (
    event: ReactKeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    let nextIndex: number | undefined;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') {
      nextIndex = (index + 1) % nodeCategories.length;
    } else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') {
      nextIndex = (index - 1 + nodeCategories.length) % nodeCategories.length;
    } else if (event.key === 'Home') {
      nextIndex = 0;
    } else if (event.key === 'End') {
      nextIndex = nodeCategories.length - 1;
    }
    if (nextIndex === undefined) return;
    event.preventDefault();
    const nextCategory = nodeCategories[nextIndex];
    selectCategory(nextCategory.id);
    categoryRefs.current[nextIndex]?.focus();
  };

  const updateValue = (fieldId: string, value: ScalarValue) => {
    setDrafts((current) => ({
      ...current,
      [activeDefinition.id]: {
        ...(current[activeDefinition.id] ??
          initialNodeValues(activeDefinition)),
        [fieldId]: value,
      },
    }));
    setCopyState('idle');
  };

  const updateRepeater = (
    fieldId: string,
    index: number,
    itemFieldId: string,
    value: ScalarValue,
  ) => {
    setDrafts((current) => {
      const draft =
        current[activeDefinition.id] ?? initialNodeValues(activeDefinition);
      const items = draft[fieldId];
      if (!Array.isArray(items)) return current;
      return {
        ...current,
        [activeDefinition.id]: {
          ...draft,
          [fieldId]: items.map((item, itemIndex) =>
            itemIndex === index ? { ...item, [itemFieldId]: value } : item,
          ),
        },
      };
    });
    setCopyState('idle');
  };

  const addRepeaterItem = (field: NodeRepeaterField) => {
    setDrafts((current) => {
      const draft =
        current[activeDefinition.id] ?? initialNodeValues(activeDefinition);
      const items = draft[field.id];
      const currentItems = Array.isArray(items) ? items : [];
      if (
        field.maxItems !== undefined &&
        currentItems.length >= field.maxItems
      ) {
        return current;
      }
      const next = Object.fromEntries(
        field.itemFields.map((itemField) => [
          itemField.id,
          itemField.defaultValue,
        ]),
      );
      return {
        ...current,
        [activeDefinition.id]: {
          ...draft,
          [field.id]: [...currentItems, next],
        },
      };
    });
    setCopyState('idle');
  };

  const removeRepeaterItem = (field: NodeRepeaterField, index: number) => {
    setDrafts((current) => {
      const draft =
        current[activeDefinition.id] ?? initialNodeValues(activeDefinition);
      const items = draft[field.id];
      if (!Array.isArray(items) || items.length <= field.minItems) {
        return current;
      }
      return {
        ...current,
        [activeDefinition.id]: {
          ...draft,
          [field.id]: items.filter((_, itemIndex) => itemIndex !== index),
        },
      };
    });
    setCopyState('idle');
  };

  const reset = () => {
    setDrafts((current) => ({
      ...current,
      [activeDefinition.id]: initialNodeValues(activeDefinition),
    }));
    setCopyState('idle');
  };

  const copyJson = async () => {
    try {
      await writeClipboard(result.json);
      setCopyState('copied');
    } catch {
      setCopyState('error');
    }
  };

  return (
    <section
      aria-label={copy.formLabel}
      className="flow-node-config rp-not-doc"
      data-node-config-lab
    >
      <header className="flow-node-config__header">
        <div>
          <span className="flow-node-config__eyebrow">{copy.versionNote}</span>
          <h3>{copy.formLabel}</h3>
        </div>
        <span className="flow-node-config__count">{copy.coverage}</span>
      </header>

      <div
        aria-label={copy.catalogLabel}
        className="flow-node-config__categories"
        role="tablist"
      >
        {nodeCategories.map((category, index) => (
          <button
            aria-controls="flow-node-config-panel"
            aria-selected={activeCategory === category.id}
            className="flow-node-config__category"
            id={`flow-node-category-${category.id}`}
            key={category.id}
            onClick={() => selectCategory(category.id)}
            onKeyDown={(event) => handleCategoryKeyDown(event, index)}
            ref={(element) => {
              categoryRefs.current[index] = element;
            }}
            role="tab"
            tabIndex={activeCategory === category.id ? 0 : -1}
            type="button"
          >
            <span>{localized(category.label, locale)}</span>
            <small>
              {
                nodeDefinitions.filter(
                  (definition) => definition.category === category.id,
                ).length
              }
            </small>
          </button>
        ))}
      </div>

      <div
        aria-labelledby={`flow-node-category-${activeCategory}`}
        className="flow-node-config__layout"
        id="flow-node-config-panel"
        role="tabpanel"
      >
        <nav aria-label={copy.catalogLabel} className="flow-node-config__nodes">
          {visibleNodes.map((definition) => (
            <button
              aria-current={definition.id === activeDefinition.id}
              className="flow-node-config__node"
              key={definition.id}
              onClick={() => selectNode(definition)}
              type="button"
            >
              <span>{localized(definition.label, locale)}</span>
              <code>{definition.wireType}</code>
            </button>
          ))}
        </nav>

        <div className="flow-node-config__content">
          <header className="flow-node-config__node-header">
            <div>
              <span className="flow-node-config__node-title">
                {localized(activeDefinition.label, locale)}
                <code>{activeDefinition.wireType}</code>
              </span>
              <p>{localized(activeDefinition.summary, locale)}</p>
            </div>
            <button
              className="flow-node-config__quiet-action"
              onClick={reset}
              type="button"
            >
              <ArrowCounterClockwise aria-hidden="true" size={16} />
              {copy.reset}
            </button>
          </header>

          <div className="flow-node-config__editor">
            <form
              className="flow-node-config__form"
              onSubmit={(event) => event.preventDefault()}
            >
              <p className="flow-node-config__form-note">{copy.requiredHint}</p>
              {activeDefinition.sections.map((section) => (
                <fieldset
                  className="flow-node-config__section"
                  key={section.title.en}
                >
                  <legend>{localized(section.title, locale)}</legend>
                  <div className="flow-node-config__fields">
                    {section.fields.map((field) => (
                      <ConfigField
                        field={field}
                        fieldErrors={result.fieldErrors}
                        key={field.id}
                        locale={locale}
                        onAddItem={addRepeaterItem}
                        onChange={updateValue}
                        onRemoveItem={removeRepeaterItem}
                        onRepeaterChange={updateRepeater}
                        values={values}
                      />
                    ))}
                  </div>
                </fieldset>
              ))}
            </form>

            <aside className="flow-node-config__preview">
              <header
                data-state={result.errors.length === 0 ? 'valid' : 'invalid'}
              >
                <div>
                  <span>
                    {activeDefinition.outputKind === 'graph'
                      ? copy.graphPreview
                      : copy.preview}
                  </span>
                  <small>
                    {result.errors.length === 0 ? copy.valid : copy.invalid}
                  </small>
                </div>
                <button
                  className="flow-node-config__copy"
                  disabled={result.errors.length > 0}
                  onClick={copyJson}
                  type="button"
                >
                  {copyState === 'copied' ? (
                    <Check aria-hidden="true" size={16} />
                  ) : (
                    <Copy aria-hidden="true" size={16} />
                  )}
                  {copyState === 'copied'
                    ? copy.copied
                    : copyState === 'error'
                      ? copy.copyFailed
                      : copy.copy}
                </button>
                <output
                  aria-live="polite"
                  className="flow-node-config__sr-only"
                >
                  {copyState === 'copied'
                    ? copy.copied
                    : copyState === 'error'
                      ? copy.copyFailed
                      : ''}
                </output>
              </header>

              {result.errors.length > 0 && (
                <ul aria-live="polite" className="flow-node-config__errors">
                  {result.errors.map((error) => (
                    <li key={error}>
                      <WarningCircle aria-hidden="true" size={15} />
                      {error}
                    </li>
                  ))}
                </ul>
              )}

              <pre aria-label={copy.preview} tabIndex={0}>
                <code>{result.json}</code>
              </pre>
            </aside>
          </div>
        </div>
      </div>
    </section>
  );
}

type ConfigFieldProps = {
  field: NodeConfigField;
  fieldErrors: Record<string, string[]>;
  locale: NodeConfigLocale;
  onAddItem: (field: NodeRepeaterField) => void;
  onChange: (fieldId: string, value: ScalarValue) => void;
  onRemoveItem: (field: NodeRepeaterField, index: number) => void;
  onRepeaterChange: (
    fieldId: string,
    index: number,
    itemFieldId: string,
    value: ScalarValue,
  ) => void;
  values: NodeFormValues;
};

function ConfigField({
  field,
  fieldErrors,
  locale,
  onAddItem,
  onChange,
  onRemoveItem,
  onRepeaterChange,
  values,
}: ConfigFieldProps) {
  if (field.kind === 'repeater') {
    const items = values[field.id];
    return (
      <div className="flow-node-config__repeater">
        <div className="flow-node-config__repeater-heading">
          <div>
            <span>{localized(field.label, locale)}</span>
            <small>{localized(field.help, locale)}</small>
          </div>
          {Array.isArray(items) && (
            <span className="flow-node-config__repeater-count">
              {items.length}
              {field.maxItems !== undefined ? ` / ${field.maxItems}` : ''}
            </span>
          )}
          <button
            disabled={
              field.maxItems !== undefined &&
              Array.isArray(items) &&
              items.length >= field.maxItems
            }
            onClick={() => onAddItem(field)}
            type="button"
          >
            <Plus aria-hidden="true" size={15} />
            {nodeConfigCopy[locale].addItem}
          </button>
        </div>
        <FieldErrors errors={fieldErrors[field.id]} />
        <div className="flow-node-config__repeater-items">
          {Array.isArray(items) &&
            items.map((item, index) => (
              <fieldset
                className="flow-node-config__repeater-item"
                key={`${field.id}-${index}`}
              >
                <legend>
                  {localized(field.itemLabel, locale)} {index + 1}
                </legend>
                <button
                  aria-label={`${nodeConfigCopy[locale].removeItem} ${index + 1}`}
                  className="flow-node-config__remove"
                  disabled={items.length <= field.minItems}
                  onClick={() => onRemoveItem(field, index)}
                  type="button"
                >
                  <Trash aria-hidden="true" size={15} />
                </button>
                <div className="flow-node-config__item-fields">
                  {field.itemFields.map((itemField) => (
                    <ScalarField
                      field={itemField}
                      idPrefix={`${field.id}-${index}`}
                      key={itemField.id}
                      locale={locale}
                      onChange={(value) =>
                        onRepeaterChange(field.id, index, itemField.id, value)
                      }
                      errors={
                        fieldErrors[`${field.id}.${index}.${itemField.id}`]
                      }
                      value={item[itemField.id] ?? itemField.defaultValue}
                    />
                  ))}
                </div>
              </fieldset>
            ))}
        </div>
      </div>
    );
  }

  if (!isFieldVisible(field, values)) return null;
  return (
    <ScalarField
      field={field}
      idPrefix="node"
      locale={locale}
      onChange={(value) => onChange(field.id, value)}
      errors={fieldErrors[field.id]}
      value={(values[field.id] as ScalarValue) ?? field.defaultValue}
    />
  );
}

type ScalarFieldProps = {
  errors?: string[];
  field: NodeScalarField;
  idPrefix: string;
  locale: NodeConfigLocale;
  onChange: (value: ScalarValue) => void;
  value: ScalarValue;
};

function ScalarField({
  errors,
  field,
  idPrefix,
  locale,
  onChange,
  value,
}: ScalarFieldProps) {
  const id = `flow-node-${idPrefix}-${field.id}`;
  const label = localized(field.label, locale);
  const help = localized(field.help, locale);
  const helpId = `${id}-help`;
  const errorId = `${id}-error`;
  const describedBy = [helpId, errors?.length ? errorId : '']
    .filter(Boolean)
    .join(' ');

  if (field.kind === 'switch') {
    return (
      <div className="flow-node-config__switch-wrap">
        <label className="flow-node-config__switch" htmlFor={id}>
          <span>
            <strong>{label}</strong>
            <small id={helpId}>{help}</small>
          </span>
          <input
            aria-describedby={describedBy}
            aria-invalid={Boolean(errors?.length)}
            checked={Boolean(value)}
            id={id}
            onChange={(event) => onChange(event.target.checked)}
            role="switch"
            type="checkbox"
          />
        </label>
        <FieldErrors errors={errors} id={errorId} />
      </div>
    );
  }

  return (
    <div
      className={`flow-node-config__field flow-node-config__field--${field.kind}`}
    >
      <label htmlFor={id}>
        {label}
        {field.required && (
          <>
            <b aria-hidden="true">*</b>
            <span className="flow-node-config__sr-only">
              {nodeConfigCopy[locale].required}
            </span>
          </>
        )}
      </label>
      {field.kind === 'select' ? (
        <select
          aria-describedby={describedBy}
          aria-invalid={Boolean(errors?.length)}
          id={id}
          onChange={(event) => onChange(event.target.value)}
          value={String(value)}
        >
          {field.options?.map((entry) => (
            <option key={entry.value} value={entry.value}>
              {localized(entry.label, locale)}
            </option>
          ))}
        </select>
      ) : field.kind === 'textarea' || field.kind === 'json' ? (
        <textarea
          aria-describedby={describedBy}
          aria-invalid={Boolean(errors?.length)}
          id={id}
          onChange={(event) => onChange(event.target.value)}
          rows={field.kind === 'json' ? 5 : 3}
          spellCheck={field.kind !== 'json'}
          value={String(value)}
        />
      ) : (
        <input
          aria-describedby={describedBy}
          aria-invalid={Boolean(errors?.length)}
          id={id}
          min={field.min}
          onChange={(event) =>
            onChange(
              field.kind === 'number'
                ? event.target.value === ''
                  ? ''
                  : event.target.valueAsNumber
                : event.target.value,
            )
          }
          type={
            field.kind === 'number'
              ? 'number'
              : field.kind === 'datetime'
                ? 'datetime-local'
                : 'text'
          }
          value={String(value)}
        />
      )}
      <small id={helpId}>{help}</small>
      <FieldErrors errors={errors} id={errorId} />
    </div>
  );
}

function FieldErrors({ errors, id }: { errors?: string[]; id?: string }) {
  if (!errors?.length) return null;
  return (
    <ul className="flow-node-config__field-errors" id={id}>
      {errors.map((error) => (
        <li key={error}>{error}</li>
      ))}
    </ul>
  );
}

async function writeClipboard(value: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }

  const fallback = document.createElement('textarea');
  fallback.value = value;
  fallback.setAttribute('readonly', '');
  fallback.style.position = 'fixed';
  fallback.style.opacity = '0';
  document.body.append(fallback);
  fallback.select();
  const copied = document.execCommand('copy');
  fallback.remove();
  if (!copied) throw new Error('Clipboard write failed');
}
