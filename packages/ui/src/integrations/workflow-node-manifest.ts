/** Numeric range metadata accepted by workflow-node fields. */
export interface WorkflowNodeRangeSpec {
  min?: number;
  max?: number;
  step?: number;
  step_type?: 'int' | 'float' | string;
}

/** Column metadata for a workflow-node table field. */
export interface WorkflowNodeTableColumn {
  name: string;
  display_name?: string;
  description?: string;
  type?: string;
  default?: unknown;
  options?: unknown[];
  edit_mode?: string;
  required?: boolean;
  [key: string]: unknown;
}

/** Host-owned field metadata used to compile a workflow-node configuration form. */
export interface WorkflowNodeFieldDefinition {
  name: string;
  /** Form-compatible widget spelling for hosts that already expose one. */
  widget?: string;
  /** Explicit Flow control override (camelCase and snake_case are accepted). */
  controlWidget?: string;
  control_widget?: string;
  _input_type?: string;
  inputType?: string;
  input_type?: string;
  type?: string;
  display_name?: string;
  info?: string;
  placeholder?: string;
  value?: unknown;
  required?: boolean;
  advanced?: boolean;
  show?: boolean;
  readonly?: boolean;
  password?: boolean;
  multiline?: boolean;
  list?: boolean;
  is_list?: boolean;
  tool_mode?: boolean;
  dynamic?: boolean;
  real_time_refresh?: boolean;
  refresh_button?: boolean;
  combobox?: boolean;
  options?: unknown[];
  /** JSON-Schema-style enum accepted by hosts that do not use `options`. */
  enum?: unknown[];
  /** Common host spelling for an option list. */
  choices?: unknown[];
  values?: unknown[];
  /** Optional nested schema supplied by form-oriented hosts. */
  schema?: {
    enum?: unknown[];
    options?: unknown[];
    choices?: unknown[];
    values?: unknown[];
    items?: { enum?: unknown[]; options?: unknown[]; [key: string]: unknown };
    [key: string]: unknown;
  };
  input_types?: string[];
  fileTypes?: string[];
  file_types?: string[];
  range_spec?: WorkflowNodeRangeSpec;
  rangeSpec?: WorkflowNodeRangeSpec;
  visible_when?: {
    field: string;
    equals: unknown;
  };
  table_schema?:
    WorkflowNodeTableColumn[] | { columns?: WorkflowNodeTableColumn[] };
  model_type?: string;
  [key: string]: unknown;
}

/** Typed output metadata exposed by a workflow node. */
export interface WorkflowNodeOutputDefinition {
  name: string;
  display_name: string;
  types: string[];
  selected?: string;
  group_outputs: boolean;
  allows_loop: boolean;
  tool_mode: boolean;
  info?: string;
}

/** Host-owned definition for a configurable workflow node. */
export interface WorkflowNodeDefinition {
  category: string;
  categoryLabel: string;
  type: string;
  display_name: string;
  description: string;
  icon?: string;
  documentation?: string;
  beta: boolean;
  legacy: boolean;
  official: boolean;
  tool_mode: boolean;
  base_classes: string[];
  input_types: string[];
  output_types: string[];
  fields: WorkflowNodeFieldDefinition[];
  outputs: WorkflowNodeOutputDefinition[];
}
