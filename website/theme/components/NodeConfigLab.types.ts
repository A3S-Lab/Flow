export type NodeConfigLocale = 'zh' | 'en';
export type NodeCategoryId = 'work' | 'suspension' | 'composition' | 'terminal';

export type LocalText = Record<NodeConfigLocale, string>;
export type ScalarValue = string | number | boolean;
export type RepeaterValue = Array<Record<string, ScalarValue>>;
export type NodeFormValues = Record<string, ScalarValue | RepeaterValue>;

export type ValidationIssue = {
  fieldId: string;
  message: string;
};

export type VisibilityRule = {
  field: string;
  equals?: ScalarValue;
  not?: ScalarValue;
};

export type NodeSelectOption = {
  label: LocalText;
  value: string;
};

export type NodeScalarField = {
  defaultValue: ScalarValue;
  help: LocalText;
  id: string;
  kind:
    'datetime' | 'json' | 'number' | 'select' | 'switch' | 'text' | 'textarea';
  label: LocalText;
  min?: number;
  options?: NodeSelectOption[];
  required?: boolean;
  visibleWhen?: VisibilityRule;
};

export type NodeRepeaterField = {
  defaultValue: RepeaterValue;
  help: LocalText;
  id: string;
  itemFields: NodeScalarField[];
  itemLabel: LocalText;
  kind: 'repeater';
  label: LocalText;
  maxItems?: number;
  minItems: number;
};

export type NodeConfigField = NodeRepeaterField | NodeScalarField;

export type NodeDefinition = {
  category: NodeCategoryId;
  id: string;
  label: LocalText;
  outputKind: 'command' | 'graph';
  sections: Array<{
    fields: NodeConfigField[];
    title: LocalText;
  }>;
  summary: LocalText;
  wireType: string;
};
