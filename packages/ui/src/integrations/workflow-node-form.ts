import type {
  ActionDefinition,
  FormDocument,
  JsonObject,
  JsonPrimitive,
  JsonSchema,
  JsonValue,
  UiNode,
  UiOption,
} from "@a3s-lab/ui/form/core";
import type {
  WorkflowNodeDefinition,
  WorkflowNodeFieldDefinition,
  WorkflowNodeTableColumn,
} from "./workflow-node-manifest";

export type {
  WorkflowNodeDefinition,
  WorkflowNodeFieldDefinition,
  WorkflowNodeOutputDefinition,
  WorkflowNodeRangeSpec,
  WorkflowNodeTableColumn,
} from "./workflow-node-manifest";

export const WORKFLOW_CONFIGURATION_WIDGETS = Object.freeze({
  actionPicker: "a3s.workflow.action-picker",
  code: "a3s.workflow.code",
  connection: "a3s.workflow.connection",
  dataDisplay: "a3s.workflow.data-display",
  duration: "a3s.workflow.duration",
  file: "a3s.workflow.file",
  flowBatch: "a3s.flow.batch",
  flowChildren: "a3s.flow.children",
  flowExpression: "a3s.flow.expression",
  flowSchema: "a3s.flow.schema",
  flowSpec: "a3s.flow.spec",
  json: "a3s.workflow.json",
  mcp: "a3s.workflow.mcp",
  model: "a3s.workflow.model",
  parameter: "a3s.workflow.parameter",
  prompt: "a3s.workflow.prompt",
  sortableList: "a3s.workflow.sortable-list",
  tabs: "a3s.workflow.tabs",
});

export const WORKFLOW_CONFIGURATION_WIDGET_KEYS = Object.freeze(
  Object.values(WORKFLOW_CONFIGURATION_WIDGETS),
);

export const WORKFLOW_NODE_FIELD_GROUPS = Object.freeze({
  input: "Input",
  "provider-credentials": "Provider & credentials",
  behavior: "Behavior",
  runtime: "Runtime",
  output: "Output",
  "storage-retrieval": "Storage & retrieval",
} as const);

export type WorkflowNodeFieldGroup = keyof typeof WORKFLOW_NODE_FIELD_GROUPS;

export interface CreateWorkflowNodeFormOptions {
  locale?: string;
  presentation?: "catalog" | "task";
  fieldVisibility?: Readonly<Record<string, boolean>>;
  buildConfig?: Readonly<Record<string, WorkflowNodeFieldDefinition>>;
  actions?: readonly ActionDefinition[];
  compatibility?: readonly string[];
}

/**
 * Compares JSON-like values without relying on object identity. Manifest
 * conditions commonly contain arrays or small objects, and those values are
 * cloned when a DAG node crosses the editor boundary.
 */
function equalJsonLike(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (Array.isArray(left) || Array.isArray(right)) {
    if (!Array.isArray(left) || !Array.isArray(right)) return false;
    return (
      left.length === right.length &&
      left.every((entry, index) => equalJsonLike(entry, right[index]))
    );
  }
  if (
    !left ||
    !right ||
    typeof left !== "object" ||
    typeof right !== "object"
  ) {
    return false;
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftRecord);
  const rightKeys = Object.keys(rightRecord);
  return (
    leftKeys.length === rightKeys.length &&
    leftKeys.every(
      (key) =>
        Object.hasOwn(rightRecord, key) &&
        equalJsonLike(leftRecord[key], rightRecord[key]),
    )
  );
}

/**
 * Resolves whether a manifest field should be presented for the current
 * configuration. Explicit host visibility overrides take precedence over the
 * static `show` flag and `visible_when` condition.
 */
export function isWorkflowNodeFieldVisible(
  field: WorkflowNodeFieldDefinition,
  value?: Readonly<Record<string, unknown>>,
  fieldVisibility?: Readonly<Record<string, boolean>>,
): boolean {
  if (fieldVisibility && Object.hasOwn(fieldVisibility, field.name)) {
    return fieldVisibility[field.name] === true;
  }
  if (field.show === false) return false;
  const condition = field.visible_when;
  // A form document may be compiled before a value exists. In that case keep
  // conditional fields available; the host can provide a visibility map when
  // it has a live DAG configuration.
  if (!condition || value === undefined) return true;
  if (!Object.hasOwn(value, condition.field)) return false;
  return equalJsonLike(value[condition.field], condition.equals);
}

function isChineseLocale(locale: string | undefined): boolean {
  return locale?.toLocaleLowerCase().startsWith("zh") === true;
}

function defaultActions(
  locale: string | undefined,
): readonly ActionDefinition[] {
  return [
    {
      id: "apply",
      registryKey: "host.workflow-node.apply.v1",
      label: isChineseLocale(locale) ? "保存配置" : "Save changes",
      tone: "primary",
    },
  ];
}

function pointerToken(value: string): string {
  return value.replaceAll("~", "~0").replaceAll("/", "~1");
}

function isPrimitive(value: unknown): value is JsonPrimitive {
  return (
    value === null ||
    typeof value === "string" ||
    typeof value === "boolean" ||
    (typeof value === "number" && Number.isFinite(value))
  );
}

function isJsonValue(value: unknown): value is JsonValue {
  if (isPrimitive(value)) return true;
  if (Array.isArray(value)) return value.every(isJsonValue);
  if (!value || typeof value !== "object") return false;
  return Object.values(value).every(isJsonValue);
}

function fallbackValue(
  field: WorkflowNodeFieldDefinition,
  type: JsonSchema["type"],
): JsonValue {
  if (type === "array") return [];
  if (type === "object") return {};
  if (type === "boolean") return false;
  if (type === "integer" || type === "number") return 0;
  if (type === "string") return "";
  return field.type === "other" ? null : "";
}

function normalizedValue(
  value: unknown,
  type: JsonSchema["type"],
  fallback: JsonValue,
): JsonValue {
  if (type === "array") {
    return Array.isArray(value) && isJsonValue(value)
      ? structuredClone(value)
      : [];
  }
  if (type === "object") {
    return value !== null &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      isJsonValue(value)
      ? structuredClone(value)
      : {};
  }
  if (type === "boolean") return typeof value === "boolean" ? value : false;
  if (type === "integer") {
    return typeof value === "number" && Number.isInteger(value) ? value : 0;
  }
  if (type === "number") {
    return typeof value === "number" && Number.isFinite(value) ? value : 0;
  }
  if (type === "string") return typeof value === "string" ? value : "";
  return isJsonValue(value) ? structuredClone(value) : fallback;
}

function normalizedTableDefault(
  field: WorkflowNodeFieldDefinition,
  value: readonly JsonValue[],
): JsonValue[] {
  const columns = tableColumns(field);
  return value.map((row) => {
    if (row === null || typeof row !== "object" || Array.isArray(row))
      return {};
    return Object.fromEntries(
      columns.flatMap((column) => {
        if (!Object.hasOwn(row, column.name)) return [];
        const type = schemaTypeForColumn(column.type);
        return [
          [
            column.name,
            normalizedValue(row[column.name], type, fallbackValue(field, type)),
          ] as const,
        ];
      }),
    );
  });
}

export function workflowNodeFieldDefault(
  field: WorkflowNodeFieldDefinition,
): JsonValue {
  const presentation = semanticFieldPresentation(field, [field]);
  const fallback = fallbackValue(field, presentation.schemaType);
  const source = field.value === "__UNDEFINED__" ? undefined : field.value;
  const value = normalizedValue(source, presentation.schemaType, fallback);
  if (field.type === "table" && Array.isArray(value)) {
    return normalizedTableDefault(field, value);
  }
  if (Array.isArray(value) && presentation.arrayItemType) {
    return value.map((item) =>
      normalizedValue(
        item,
        presentation.arrayItemType,
        fallbackValue(field, presentation.arrayItemType),
      ),
    );
  }
  return value;
}

function tableColumns(
  field: WorkflowNodeFieldDefinition,
): readonly WorkflowNodeTableColumn[] {
  const schema = field.table_schema;
  if (Array.isArray(schema)) return schema;
  return Array.isArray(schema?.columns) ? schema.columns : [];
}

function schemaTypeForColumn(type: string | undefined): JsonSchema["type"] {
  if (type === "boolean" || type === "bool") return "boolean";
  if (type === "int" || type === "integer") return "integer";
  if (type === "float" || type === "number") return "number";
  if (type === "dict" || type === "object") return "object";
  if (type === "list" || type === "array") return "array";
  return "string";
}

function tableSchema(field: WorkflowNodeFieldDefinition): JsonSchema {
  const columns = tableColumns(field);
  const properties = Object.fromEntries(
    columns.map((column) => {
      const type = schemaTypeForColumn(column.type);
      const schema: JsonSchema = {
        type,
        title: column.display_name ?? column.name,
        description: column.description,
      };
      if (type === "string" && column.required) schema.minLength = 1;
      if (column.default !== undefined) {
        schema.default = normalizedValue(
          column.default,
          type,
          fallbackValue(field, type),
        );
      }
      if (Array.isArray(column.options) && column.options.every(isPrimitive)) {
        schema.enum = [...column.options];
      }
      if (type === "object") schema.additionalProperties = true;
      if (type === "array") schema.items = {};
      return [column.name, schema];
    }),
  );
  const required = columns
    .filter((column) => column.required)
    .map((column) => column.name);
  return {
    type: "array",
    title: field.display_name ?? field.name,
    description: field.info,
    default: workflowNodeFieldDefault(field),
    items: {
      type: "object",
      properties,
      required: required.length > 0 ? required : undefined,
      additionalProperties: false,
    },
  };
}

function schemaForField(field: WorkflowNodeFieldDefinition): JsonSchema {
  if (field.type === "table") return tableSchema(field);
  const presentation = semanticFieldPresentation(field, [field]);
  const type = presentation.schemaType;

  const range = field.range_spec ?? field.rangeSpec;
  const options = simpleOptions(field);
  const schema: JsonSchema = {
    type,
    title: field.display_name ?? field.name,
    description: field.info,
    default: workflowNodeFieldDefault(field),
  };
  if (type === "array") {
    schema.items = presentation.arrayItemType
      ? { type: presentation.arrayItemType }
      : {};
    if (presentation.arrayItemType === "object")
      schema.items.additionalProperties = true;
    if (options.length > 0 && schema.items.type === "string") {
      schema.items.enum = options.map((option) => option.value);
    }
    if (field.required) schema.minItems = 1;
    schema.uniqueItems = field.type !== "actionPicker";
  }
  if (type === "object") schema.additionalProperties = true;
  if (type === "string" && field.required) schema.minLength = 1;
  if (type !== "array" && type !== "object" && options.length > 0)
    schema.enum = options.map((option) => option.value);
  if ((type === "integer" || type === "number") && range) {
    if (Number.isFinite(range.min)) schema.minimum = range.min;
    if (Number.isFinite(range.max)) schema.maximum = range.max;
    if (Number.isFinite(range.step) && Number(range.step) > 0)
      schema.multipleOf = range.step;
  }
  return schema;
}

function simpleOptions(field: WorkflowNodeFieldDefinition): UiOption[] {
  if (!Array.isArray(field.options)) return [];
  return field.options.flatMap((option) => {
    if (isPrimitive(option))
      return [{ label: String(option ?? ""), value: option }];
    if (!option || typeof option !== "object") return [];
    const record = option as Record<string, unknown>;
    const value = record.value ?? record.name;
    if (!isPrimitive(value)) return [];
    const label = record.display_name ?? record.label ?? record.name ?? value;
    return [{ label: String(label ?? ""), value }];
  });
}

function isCollectionField(field: WorkflowNodeFieldDefinition): boolean {
  return (
    field.type === "table" ||
    field.type === "sortableList" ||
    field.type === "actionPicker" ||
    field.list === true ||
    field.is_list === true ||
    field._input_type === "TableInput" ||
    field._input_type === "A3SFlowChildrenInput" ||
    field._input_type === "MultiselectInput"
  );
}

function scalarSchemaTypeForField(
  field: WorkflowNodeFieldDefinition,
): JsonSchema["type"] {
  const inputType = field._input_type;
  if (field.type === "bool" || inputType === "BoolInput") return "boolean";
  if (field.type === "int" || inputType === "IntInput") return "integer";
  if (
    field.type === "float" ||
    field.type === "slider" ||
    inputType === "FloatInput" ||
    inputType === "SliderInput"
  ) {
    return "number";
  }
  if (
    field.type === "dict" ||
    field.type === "NestedDict" ||
    field.type === "mcp" ||
    field.type === "duration" ||
    inputType === "DictInput" ||
    inputType === "NestedDictInput" ||
    inputType === "JSONInput" ||
    inputType === "McpInput" ||
    inputType === "DurationInput" ||
    inputType === "A3SFlowSpecInput" ||
    inputType === "A3SFlowChildrenInput"
  ) {
    return "object";
  }
  if (field.type === "other" || field.type === "data_display") return undefined;
  return "string";
}

function isConnectionField(field: WorkflowNodeFieldDefinition): boolean {
  const inputType = field._input_type;
  return (
    field.type === "other" ||
    inputType?.endsWith("DataInput") === true ||
    inputType === "HandleInput"
  );
}

function workflowControlWidget(field: WorkflowNodeFieldDefinition): string {
  const inputType = field._input_type;
  if (inputType === "A3SFlowExpressionInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.flowExpression;
  }
  if (inputType === "A3SFlowSchemaInput")
    return WORKFLOW_CONFIGURATION_WIDGETS.flowSchema;
  if (inputType === "A3SFlowBatchInput")
    return WORKFLOW_CONFIGURATION_WIDGETS.flowBatch;
  if (inputType === "A3SFlowChildrenInput")
    return WORKFLOW_CONFIGURATION_WIDGETS.flowChildren;
  if (inputType === "A3SFlowSpecInput")
    return WORKFLOW_CONFIGURATION_WIDGETS.flowSpec;
  if (field.type === "model" || inputType === "ModelInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.model;
  }
  if (field.type === "file" || inputType === "FileInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.file;
  }
  if (field.type === "code" || inputType === "CodeInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.code;
  }
  if (
    field.type === "prompt" ||
    field.type === "mustache" ||
    field.type === "query" ||
    inputType === "PromptInput" ||
    inputType === "QueryInput"
  ) {
    return WORKFLOW_CONFIGURATION_WIDGETS.prompt;
  }
  if (
    field.type === "dict" ||
    field.type === "NestedDict" ||
    inputType === "DictInput" ||
    inputType === "NestedDictInput" ||
    inputType === "JSONInput"
  ) {
    return WORKFLOW_CONFIGURATION_WIDGETS.json;
  }
  if (field.type === "tab" || inputType === "TabInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.tabs;
  }
  if (field.type === "sortableList" || inputType === "SortableListInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.sortableList;
  }
  if (field.type === "duration" || inputType === "DurationInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.duration;
  }
  if (field.type === "actionPicker" || inputType === "ActionPickerInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.actionPicker;
  }
  if (field.type === "mcp" || inputType === "McpInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.mcp;
  }
  if (field.type === "data_display" || inputType === "DataDisplayInput") {
    return WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay;
  }
  if (field.type === "slider" || inputType === "SliderInput") return "slider";
  if (field.type === "bool" || inputType === "BoolInput") return "switch";
  if (
    field.type === "int" ||
    field.type === "float" ||
    inputType === "IntInput" ||
    inputType === "FloatInput"
  ) {
    return "number";
  }
  if (inputType === "SecretStrInput" || field.password) return "password";
  if (inputType === "MultiselectInput") return "multi-select";
  if (inputType === "DropdownInput") return "select";
  if (inputType === "MultilineInput") return "textarea";
  if (inputType === "StrInput") {
    if (field.list || field.is_list) {
      return simpleOptions(field).length > 0 ? "multi-select" : "tags";
    }
    if (simpleOptions(field).length > 0) return "select";
    return field.multiline ? "textarea" : "text";
  }
  if (isConnectionField(field))
    return WORKFLOW_CONFIGURATION_WIDGETS.connection;
  if (field.list && simpleOptions(field).length > 0) return "multi-select";
  if (field.list || field.is_list) return "tags";
  if (simpleOptions(field).length > 0) return "select";
  if (field.multiline) return "textarea";
  return "text";
}

/** Returns the canonical A3S UI control selected for a workflow field. */
export function workflowNodeFieldControl(
  field: WorkflowNodeFieldDefinition,
): string {
  return workflowControlWidget(field);
}

function semanticName(field: WorkflowNodeFieldDefinition): string {
  return `${field.name} ${field.display_name ?? ""}`
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .toLocaleLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

function semanticGroupForField(
  field: WorkflowNodeFieldDefinition,
  controlWidget: string,
): WorkflowNodeFieldGroup {
  const name = semanticName(field);
  const terms = new Set(name.split("_"));
  const hasAny = (...values: string[]) =>
    values.some((value) => terms.has(value));
  const booleanControl = field.type === "bool" || controlWidget === "switch";

  if (field.type === "data_display" || field.readonly === true) return "output";
  if (controlWidget === WORKFLOW_CONFIGURATION_WIDGETS.connection)
    return "input";
  if (booleanControl) return "behavior";
  if (
    controlWidget === WORKFLOW_CONFIGURATION_WIDGETS.model ||
    controlWidget === "password" ||
    field.password === true ||
    hasAny(
      "provider",
      "credential",
      "credentials",
      "password",
      "secret",
      "endpoint",
      "deployment",
    ) ||
    hasAny("model") ||
    (hasAny("key", "token") &&
      hasAny("api", "access", "auth", "bearer", "service"))
  ) {
    return "provider-credentials";
  }
  if (
    field.type === "duration" ||
    hasAny(
      "timeout",
      "timeouts",
      "retry",
      "retries",
      "concurrency",
      "worker",
      "workers",
      "parallel",
      "batch",
      "delay",
      "duration",
      "interval",
      "polling",
      "ttl",
      "limit",
      "limits",
    ) ||
    ((field.type === "int" ||
      field.type === "float" ||
      field.type === "slider") &&
      hasAny("min", "max", "minimum", "maximum"))
  ) {
    return "runtime";
  }
  if (
    hasAny(
      "output",
      "outputs",
      "result",
      "results",
      "response",
      "destination",
      "return",
    )
  ) {
    return "output";
  }
  if (
    hasAny(
      "storage",
      "database",
      "db",
      "vectorstore",
      "index",
      "collection",
      "namespace",
      "bucket",
      "directory",
      "folder",
      "cache",
      "memory",
      "retriever",
      "retrieval",
    ) ||
    (hasAny("path") && !hasAny("select", "selected"))
  ) {
    return "storage-retrieval";
  }
  if (
    field.type === "bool" ||
    field.type === "int" ||
    field.type === "float" ||
    field.type === "slider" ||
    controlWidget === "switch" ||
    controlWidget === "number" ||
    controlWidget === "slider" ||
    controlWidget === "select" ||
    controlWidget === WORKFLOW_CONFIGURATION_WIDGETS.tabs ||
    hasAny(
      "mode",
      "method",
      "strategy",
      "behavior",
      "operation",
      "format",
      "config",
      "settings",
    )
  ) {
    return "behavior";
  }
  return "input";
}

export function workflowNodeFieldGroup(
  field: WorkflowNodeFieldDefinition,
): WorkflowNodeFieldGroup {
  return semanticGroupForField(field, workflowControlWidget(field));
}

interface SemanticFieldPresentation {
  schemaType: JsonSchema["type"];
  arrayItemType: JsonSchema["type"];
  controlWidget: string;
  semanticGroup: WorkflowNodeFieldGroup;
  width: UiNode["width"];
}

function semanticFieldPresentation(
  field: WorkflowNodeFieldDefinition,
  fields: readonly WorkflowNodeFieldDefinition[],
): SemanticFieldPresentation {
  const collection = isCollectionField(field);
  const scalarType = scalarSchemaTypeForField(field);
  const schemaType = collection ? "array" : scalarType;
  const controlWidget = workflowControlWidget(field);
  const semanticGroup = semanticGroupForField(field, controlWidget);
  let arrayItemType: JsonSchema["type"];
  if (
    field.type === "table" ||
    field._input_type === "TableInput" ||
    field._input_type === "A3SFlowChildrenInput"
  )
    arrayItemType = "object";
  else if (
    collection &&
    field.type !== "sortableList" &&
    field.type !== "actionPicker" &&
    field._input_type !== "JSONInput" &&
    controlWidget !== WORKFLOW_CONFIGURATION_WIDGETS.connection
  ) {
    arrayItemType = scalarType;
  }

  const fullWidth =
    field.type === "table" ||
    schemaType === "array" ||
    field.multiline === true ||
    field._input_type === "MultilineInput" ||
    (
      [
        WORKFLOW_CONFIGURATION_WIDGETS.connection,
        WORKFLOW_CONFIGURATION_WIDGETS.code,
        WORKFLOW_CONFIGURATION_WIDGETS.dataDisplay,
        WORKFLOW_CONFIGURATION_WIDGETS.file,
        WORKFLOW_CONFIGURATION_WIDGETS.flowChildren,
        WORKFLOW_CONFIGURATION_WIDGETS.flowSpec,
        WORKFLOW_CONFIGURATION_WIDGETS.json,
        WORKFLOW_CONFIGURATION_WIDGETS.mcp,
        WORKFLOW_CONFIGURATION_WIDGETS.prompt,
        WORKFLOW_CONFIGURATION_WIDGETS.sortableList,
        WORKFLOW_CONFIGURATION_WIDGETS.actionPicker,
      ] as readonly string[]
    ).includes(controlWidget);
  const runtimeNumericFields = fields.filter((candidate) => {
    const type = scalarSchemaTypeForField(candidate);
    const control = workflowControlWidget(candidate);
    return (
      candidate.show !== false &&
      (candidate.advanced === true) === (field.advanced === true) &&
      !isCollectionField(candidate) &&
      (type === "integer" || type === "number") &&
      semanticGroupForField(candidate, control) === "runtime"
    );
  });
  const compactRuntime =
    !fullWidth &&
    (schemaType === "integer" || schemaType === "number") &&
    semanticGroup === "runtime" &&
    runtimeNumericFields.length >= 3;

  return {
    schemaType,
    arrayItemType,
    controlWidget,
    semanticGroup,
    width: fullWidth ? 12 : compactRuntime ? 4 : 6,
  };
}

function workflowWidget(control: string): string {
  if (
    control === WORKFLOW_CONFIGURATION_WIDGETS.flowExpression ||
    control === WORKFLOW_CONFIGURATION_WIDGETS.flowSchema ||
    control === WORKFLOW_CONFIGURATION_WIDGETS.flowBatch ||
    control === WORKFLOW_CONFIGURATION_WIDGETS.flowChildren ||
    control === WORKFLOW_CONFIGURATION_WIDGETS.flowSpec
  ) {
    return control;
  }
  return control === WORKFLOW_CONFIGURATION_WIDGETS.connection
    ? control
    : WORKFLOW_CONFIGURATION_WIDGETS.parameter;
}

function fieldCustomProps(
  field: WorkflowNodeFieldDefinition,
  presentation: SemanticFieldPresentation,
): JsonObject {
  const props: JsonObject = {
    inputType: field._input_type ?? "",
    sourceType: field.type ?? "",
    inputTypes: field.input_types ?? [],
    toolMode: field.tool_mode === true,
    dynamic: field.dynamic === true,
    realTimeRefresh: field.real_time_refresh === true,
    refreshButton: field.refresh_button === true,
    advanced: field.advanced === true,
    combobox: field.combobox === true,
    controlWidget: presentation.controlWidget,
    semanticGroup: presentation.semanticGroup,
    semanticGroupLabel: WORKFLOW_NODE_FIELD_GROUPS[presentation.semanticGroup],
  };
  if (Array.isArray(field.options) && isJsonValue(field.options)) {
    props.sourceOptions = structuredClone(field.options);
  }
  const metadata: ReadonlyArray<readonly [string, string]> = [
    ["listAddLabel", "list_add_label"],
    ["externalOptions", "external_options"],
    ["optionsMetadata", "options_metadata"],
    ["dialogInputs", "dialog_inputs"],
    ["copyField", "copy_field"],
    ["triggerText", "trigger_text"],
    ["triggerIcon", "trigger_icon"],
    ["tableIcon", "table_icon"],
    ["filePath", "file_path"],
    ["language", "language"],
    ["tempFile", "temp_file"],
    ["limit", "limit"],
    ["sliderInput", "slider_input"],
    ["sliderButtons", "slider_buttons"],
    ["sliderButtonsOptions", "slider_buttons_options"],
    ["minLabel", "min_label"],
    ["minLabelIcon", "min_label_icon"],
    ["maxLabel", "max_label"],
    ["maxLabelIcon", "max_label_icon"],
    ["buttonText", "button_text"],
    ["buttonIcon", "button_icon"],
    ["searchCategory", "search_category"],
  ];
  for (const [target, source] of metadata) {
    const value = field[source];
    if (value !== undefined && isJsonValue(value))
      props[target] = structuredClone(value);
  }
  const range = field.range_spec ?? field.rangeSpec;
  if (
    typeof range?.step === "number" &&
    Number.isFinite(range.step) &&
    range.step > 0
  ) {
    props.step = range.step;
  }
  const fileTypes = field.fileTypes ?? field.file_types;
  if (Array.isArray(fileTypes)) props.fileTypes = [...fileTypes];
  if (typeof field.model_type === "string") props.modelType = field.model_type;
  if (typeof field.expression_purpose === "string") {
    props.expressionPurpose = field.expression_purpose;
  }
  return props;
}

function fieldId(type: string, name: string): string {
  return `${type}-${name}`
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLocaleLowerCase();
}

function tableFieldNodes(
  node: WorkflowNodeDefinition,
  field: WorkflowNodeFieldDefinition,
  fields: readonly WorkflowNodeFieldDefinition[],
): UiNode[] {
  const id = fieldId(node.type, field.name);
  const path = `/properties/${pointerToken(field.name)}`;
  const columns = tableColumns(field);
  const children = columns.map(
    (column) => `${id}-${fieldId("column", column.name)}`,
  );
  const presentation = semanticFieldPresentation(field, fields);
  return [
    {
      id,
      kind: "repeater",
      label: field.display_name ?? field.name,
      description: field.info,
      schemaPath: path,
      width: 12,
      layout: "data-grid",
      dataGrid: {
        editMode: "dialog",
        selection: "multiple",
        sorting: "single",
        filtering: "search",
        paste: "append",
        fill: "down",
      },
      children,
      hidden: field.show === false,
      customProps: fieldCustomProps(field, presentation),
    },
    ...columns.map((column, index) => ({
      id: children[index],
      kind: "field" as const,
      label: column.display_name ?? column.name,
      description: column.description,
      schemaPath: `${path}/items/properties/${pointerToken(column.name)}`,
      widget:
        schemaTypeForColumn(column.type) === "boolean"
          ? "switch"
          : schemaTypeForColumn(column.type) === "number" ||
              schemaTypeForColumn(column.type) === "integer"
            ? "number"
            : Array.isArray(column.options)
              ? "select"
              : "text",
      options: Array.isArray(column.options)
        ? column.options
            .filter(isPrimitive)
            .map((value) => ({ label: String(value), value }))
        : undefined,
      width: Math.max(
        3,
        Math.floor(12 / Math.max(1, Math.min(columns.length, 4))),
      ) as UiNode["width"],
    })),
  ];
}

/**
 * Returns the manifest fields in declaration order and applies host-side
 * build-config overrides by field name. Overrides are shallow-merged so a
 * caller can change one presentation property without accidentally dropping
 * the field's type, default, validation, or semantic metadata. New names are
 * retained as explicit host extensions and appended after manifest fields.
 */
export function resolveWorkflowNodeFields(
  node: WorkflowNodeDefinition,
  options: CreateWorkflowNodeFormOptions = {},
  value?: Readonly<Record<string, unknown>>,
): WorkflowNodeFieldDefinition[] {
  const fieldsByName = new Map<string, WorkflowNodeFieldDefinition>();
  const order: string[] = [];
  for (const field of node.fields) {
    fieldsByName.set(field.name, { ...field });
    order.push(field.name);
  }
  for (const [name, override] of Object.entries(options.buildConfig ?? {})) {
    if (override.name !== name) {
      throw new TypeError(
        `Workflow build-config key ${name} must match field name ${override.name}.`,
      );
    }
    const base = fieldsByName.get(name);
    fieldsByName.set(name, base ? { ...base, ...override } : { ...override });
    if (!base) order.push(name);
  }
  return order.map((name) => {
    const field = fieldsByName.get(name);
    if (!field) throw new Error(`Missing workflow field ${name}.`);
    const visible = isWorkflowNodeFieldVisible(
      field,
      value,
      options.fieldVisibility,
    );
    return visible === (field.show !== false)
      ? field
      : { ...field, show: visible };
  });
}

function fieldsForNode(
  node: WorkflowNodeDefinition,
  options: CreateWorkflowNodeFormOptions,
): WorkflowNodeFieldDefinition[] {
  return resolveWorkflowNodeFields(node, options);
}

interface SemanticFieldRun {
  id: string;
  label: string;
  semanticGroup: WorkflowNodeFieldGroup;
  fields: WorkflowNodeFieldDefinition[];
}

function semanticFieldRuns(
  node: WorkflowNodeDefinition,
  fields: readonly WorkflowNodeFieldDefinition[],
  scope: "parameters" | "advanced",
  locale?: string,
): SemanticFieldRun[] {
  const runs: SemanticFieldRun[] = [];
  const runsByGroup = new Map<string, SemanticFieldRun>();
  for (const field of fields) {
    const semanticGroup = workflowNodeFieldGroup(field);
    const configuredGroup =
      typeof field.ui_group === "string" && field.ui_group.length > 0
        ? field.ui_group
        : semanticGroup;
    const existing = runsByGroup.get(configuredGroup);
    if (existing) {
      existing.fields.push(field);
      continue;
    }
    const configuredLabel =
      typeof field.ui_group_label === "string" &&
      field.ui_group_label.length > 0
        ? field.ui_group_label
        : undefined;
    const run = {
      id: `${fieldId(node.type, scope)}-${configuredGroup}-${runs.length + 1}`,
      label:
        configuredLabel ??
        (isChineseLocale(locale)
          ? {
              input: "主要设置",
              "provider-credentials": "服务与凭据",
              behavior: "执行方式",
              runtime: "失败与重试",
              output: "输出设置",
              "storage-retrieval": "存储与检索",
            }[semanticGroup]
          : WORKFLOW_NODE_FIELD_GROUPS[semanticGroup]),
      semanticGroup,
      fields: [field],
    } satisfies SemanticFieldRun;
    runs.push(run);
    runsByGroup.set(configuredGroup, run);
  }
  return runs;
}

function semanticGroupNode(
  node: WorkflowNodeDefinition,
  run: SemanticFieldRun,
): UiNode {
  return {
    id: run.id,
    kind: "group",
    label: run.label,
    layout: "flow",
    columns: 12,
    gap: 12,
    width: 12,
    hidden: run.fields.every((field) => field.show === false),
    customProps: {
      semanticGroupContainer: run.semanticGroup,
      semanticGroupLabel: run.label,
    },
    children: run.fields.map((field) => fieldId(node.type, field.name)),
  };
}

export function createWorkflowNodeDefaultValue(
  node: WorkflowNodeDefinition,
  options: CreateWorkflowNodeFormOptions = {},
): JsonObject {
  return Object.fromEntries(
    fieldsForNode(node, options).map((field) => [
      field.name,
      workflowNodeFieldDefault(field),
    ]),
  );
}

export function createWorkflowNodeForm(
  node: WorkflowNodeDefinition,
  options: CreateWorkflowNodeFormOptions = {},
): FormDocument {
  const fields = fieldsForNode(node, options);
  const defaultValue = createWorkflowNodeDefaultValue(node, options);
  const properties = Object.fromEntries(
    fields.map((field) => [field.name, schemaForField(field)]),
  );
  const required = fields
    .filter(
      (field) =>
        field.required === true &&
        field.show !== false &&
        field.readonly !== true &&
        field.type !== "other",
    )
    .map((field) => field.name);
  const basicFields = fields.filter((field) => field.advanced !== true);
  const advancedFields = fields.filter((field) => field.advanced === true);
  const basicRuns = semanticFieldRuns(
    node,
    basicFields,
    "parameters",
    options.locale,
  );
  const advancedRuns = semanticFieldRuns(
    node,
    advancedFields,
    "advanced",
    options.locale,
  );
  const basicSection = `${fieldId(node.type, "node")}-parameters`;
  const advancedSection = `${fieldId(node.type, "node")}-advanced`;
  const advancedPanel = `${advancedSection}-panel`;
  const rootId = `${fieldId(node.type, "node")}-root`;
  const taskPresentation = options.presentation === "task";
  const firstBasicRun = basicRuns[0];
  const flattenedBasicField = firstBasicRun?.fields[0];
  const flattenBasicRun =
    taskPresentation &&
    basicRuns.length === 1 &&
    firstBasicRun?.fields.length === 1 &&
    flattenedBasicField !== undefined;
  const basicChildren = taskPresentation
    ? flattenBasicRun
      ? [fieldId(node.type, flattenedBasicField.name)]
      : basicRuns.map((run) => run.id)
    : [basicSection];
  const nodes: UiNode[] = [
    {
      id: rootId,
      kind: "root",
      columns: 12,
      gap: 12,
      children: [
        ...basicChildren,
        ...(advancedFields.length > 0 ? [advancedSection] : []),
      ],
    },
  ];
  if (!taskPresentation) {
    nodes.push({
      id: basicSection,
      kind: "section",
      label:
        basicFields.length > 0
          ? isChineseLocale(options.locale)
            ? "节点设置"
            : "Parameters"
          : isChineseLocale(options.locale)
            ? "无需设置"
            : "No parameters",
      description:
        basicFields.length > 0
          ? isChineseLocale(options.locale)
            ? "填写这个节点执行时需要的信息。"
            : "Configure what this node needs to run."
          : isChineseLocale(options.locale)
            ? "这个节点没有需要填写的配置。"
            : "This node has no runtime parameters.",
      layout: "flow",
      columns: 12,
      gap: 12,
      width: 12,
      children: basicRuns.map((run) => run.id),
    });
  }
  if (!flattenBasicRun)
    nodes.push(...basicRuns.map((run) => semanticGroupNode(node, run)));
  if (advancedFields.length > 0) {
    const visibleAdvancedCount = advancedFields.filter(
      (field) => field.show !== false,
    ).length;
    nodes.push(
      {
        id: advancedSection,
        kind: "section",
        layout: "collapse",
        columns: 12,
        gap: 12,
        width: 12,
        customProps: { defaultOpen: false },
        children: [advancedPanel],
      },
      {
        id: advancedPanel,
        kind: "group",
        label: isChineseLocale(options.locale)
          ? `高级设置（${visibleAdvancedCount}）`
          : `Advanced · ${visibleAdvancedCount} optional ${visibleAdvancedCount === 1 ? "setting" : "settings"}`,
        layout: "collapse-panel",
        columns: 12,
        gap: 12,
        width: 12,
        children: advancedRuns.map((run) => run.id),
      },
      ...advancedRuns.map((run) => semanticGroupNode(node, run)),
    );
  }
  for (const field of fields) {
    const presentation = semanticFieldPresentation(field, fields);
    if (
      field.type === "table" &&
      presentation.controlWidget !== WORKFLOW_CONFIGURATION_WIDGETS.flowBatch
    ) {
      nodes.push(...tableFieldNodes(node, field, fields));
      continue;
    }
    nodes.push({
      id: fieldId(node.type, field.name),
      kind: "field",
      label: field.display_name ?? field.name,
      description: field.info,
      schemaPath: `/properties/${pointerToken(field.name)}`,
      widget: workflowWidget(presentation.controlWidget),
      placeholder: field.placeholder,
      options: simpleOptions(field),
      width: presentation.width,
      readOnly: field.readonly === true,
      hidden: field.show === false,
      customProps: fieldCustomProps(field, presentation),
    });
  }

  return {
    kind: "a3s.form",
    apiVersion: "a3s.dev/form/v1alpha1",
    metadata: {
      title: isChineseLocale(options.locale)
        ? `${node.display_name}配置`
        : `${node.display_name} configuration`,
      description: node.description,
      locale: options.locale ?? "en",
      owner: "A3S UI",
      tags: ["Workflow node", node.categoryLabel, node.type],
      compatibility: [...(options.compatibility ?? ["a3s-workflow/v1"])],
    },
    revision: 1,
    schema: {
      $schema: "https://json-schema.org/draft/2020-12/schema",
      type: "object",
      title: isChineseLocale(options.locale)
        ? `${node.display_name}配置`
        : `${node.display_name} configuration`,
      description: node.description,
      default: defaultValue,
      properties,
      required: required.length > 0 ? required : undefined,
      additionalProperties: false,
    },
    ui: { root: rootId, nodes },
    rules: [],
    dataSources: [],
    actions: (
      options.actions ??
      (taskPresentation ? [] : defaultActions(options.locale))
    ).map((action) => structuredClone(action)),
  };
}
