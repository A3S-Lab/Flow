import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useId,
  useMemo,
  useRef,
  useState,
} from 'react';
import { compileForm, type FormDocument, type FormHostAdapter, type JsonObject } from '@a3s-lab/ui/form/core';
import {
  type CreateWorkflowNodeFormOptions,
  createWorkflowNodeDefaultValue,
  createWorkflowNodeForm,
  isWorkflowNodeFieldVisible,
  resolveWorkflowNodeFields,
  WORKFLOW_CONFIGURATION_WIDGET_KEYS,
  type WorkflowNodeDefinition,
  type WorkflowNodeFieldDefinition,
} from '../integrations/workflow-node-form';
import { DesignerIcon } from './designer-icons';
import type { FormWidgetRegistry } from '@a3s-lab/ui/form/react';
import type { FormNodeRegistry } from '@a3s-lab/ui/form/react';
import { type FormNodeAccessoryContext, FormRenderer } from '@a3s-lab/ui/form/react';
import {
  createWorkflowConfigurationWidgetRegistry,
  type WorkflowConfigurationWidgetCallbacks,
  WorkflowFieldAccessory,
} from './workflow-configuration-widgets';
import { workflowNodeVisual } from './workflow-node-visual';
import { WorkflowNodeContractDetails } from './workflow-node-contract';

export interface WorkflowNodeConfigurationPanelProps {
  node: WorkflowNodeDefinition;
  value: JsonObject;
  onChange: (value: JsonObject) => void;
  onApply?: (value: JsonObject, document: FormDocument) => void | Promise<void>;
  onReset?: (value: JsonObject) => void;
  onRequestConnection?: WorkflowConfigurationWidgetCallbacks['onRequestConnection'];
  onRefreshField?: WorkflowConfigurationWidgetCallbacks['onRefreshField'];
  onCopyField?: WorkflowConfigurationWidgetCallbacks['onCopyField'];
  onDataDisplayAction?: WorkflowConfigurationWidgetCallbacks['onDataDisplayAction'];
  buildConfig?: Readonly<Record<string, WorkflowNodeFieldDefinition>>;
  fieldVisibility?: Readonly<Record<string, boolean>>;
  compatibility?: readonly string[];
  hostAdapter?: FormHostAdapter;
  nodeRegistry?: FormNodeRegistry;
  widgetRegistry?: FormWidgetRegistry;
  locale?: string;
  readOnly?: boolean;
  className?: string;
  presentation?: 'catalog' | 'task';
  onRun?: (value: JsonObject) => void | Promise<void>;
  onClose?: () => void;
  /** Whether to expose the node documentation action in the panel header. */
  showDocumentation?: boolean;
  lastRun?: ReactNode;
  title?: string;
  description?: string;
  onTitleChange?: (title: string) => void;
  onDescriptionChange?: (description: string) => void;
}

function panelCopy(locale: string | undefined) {
  const chinese = locale?.toLocaleLowerCase().startsWith('zh') === true;
  return chinese
    ? {
        reset: '恢复默认值',
        confirmReset: '确认恢复',
        reference: '节点文档',
        accepts: '输入',
        returns: '输出',
        shown: '个配置项',
        advanced: '个高级项',
        conditional: '个按条件显示',
        developerDetails: '技术信息',
        settings: '配置',
        lastRun: '运行结果',
        run: '运行节点',
        running: '正在运行节点',
        close: '关闭面板',
        panelSections: '节点面板',
        emptyRun: '运行节点后，结果会显示在这里。',
        nodeType: '类型 ID',
        runtimeBinding: 'Flow 命令',
        compileTitle: '无法打开节点配置。',
        compileHelp: '请检查节点定义中的字段类型和控件配置。',
        noSettingsTitle: '无需配置',
        noSettingsHelp: '这个节点没有可编辑项，可以直接连接或运行。',
        nodeTitle: '节点名称',
        nodeDescription: '节点说明',
        descriptionPlaceholder: '添加节点说明',
      }
    : {
        reset: 'Reset',
        confirmReset: 'Click again to reset',
        reference: 'Reference',
        accepts: 'Accepts',
        returns: 'Returns',
        shown: 'shown',
        advanced: 'advanced',
        conditional: 'conditional',
        developerDetails: 'Developer details',
        settings: 'Settings',
        lastRun: 'Last run',
        run: 'Run node',
        running: 'Running node',
        close: 'Close panel',
        panelSections: 'Node panel',
        emptyRun: 'Run this node to inspect its latest result.',
        nodeType: 'Node type',
        runtimeBinding: 'Runtime binding',
        compileTitle: 'The node configuration could not be opened.',
        compileHelp: 'Check the field types and control settings in the node definition.',
        noSettingsTitle: 'No configuration required',
        noSettingsHelp: 'This node has no editable parameters and is ready to connect or run.',
        nodeTitle: 'Node title',
        nodeDescription: 'Node description',
        descriptionPlaceholder: 'Add a node description',
      };
}

function uniqueTypes(values: readonly string[]): string[] {
  return [...new Set(values.filter(Boolean))];
}

function panelInputTypes(node: WorkflowNodeDefinition): string[] {
  return uniqueTypes([
    ...node.input_types,
    ...node.fields.flatMap((field) => field.input_types ?? []),
  ]);
}

function panelOutputTypes(node: WorkflowNodeDefinition): string[] {
  return uniqueTypes([...node.output_types, ...node.outputs.flatMap((output) => output.types)]);
}

export function WorkflowNodeConfigurationPanel(props: WorkflowNodeConfigurationPanelProps) {
  const copy = panelCopy(props.locale);
  const taskPresentation = props.presentation === 'task';
  const [resetPending, setResetPending] = useState(false);
  const [activeTab, setActiveTab] = useState<'settings' | 'last-run'>('settings');
  const [running, setRunning] = useState(false);
  const settingsTabRef = useRef<HTMLButtonElement>(null);
  const lastRunTabRef = useRef<HTMLButtonElement>(null);
  const panelId = useId();
  const formOptions = useMemo<CreateWorkflowNodeFormOptions>(
    () => ({
      locale: props.locale,
      presentation: props.presentation,
      buildConfig: props.buildConfig,
      fieldVisibility: props.fieldVisibility,
      compatibility: props.compatibility,
    }),
    [
      props.buildConfig,
      props.compatibility,
      props.fieldVisibility,
      props.locale,
      props.presentation,
    ],
  );
  const document = useMemo(
    () => createWorkflowNodeForm(props.node, formOptions),
    [formOptions, props.node],
  );
  const compilation = useMemo(
    () =>
      compileForm(document, {
        capabilities: {
          widgets: [
            ...new Set([
              ...WORKFLOW_CONFIGURATION_WIDGET_KEYS,
              ...Object.keys(props.widgetRegistry ?? {}),
            ]),
          ],
        },
      }),
    [document, props.widgetRegistry],
  );
  const workflowCallbacksRef = useRef<WorkflowConfigurationWidgetCallbacks>({});
  workflowCallbacksRef.current = {
    onRequestConnection: props.onRequestConnection,
    onRefreshField: props.onRefreshField,
    onCopyField: props.onCopyField,
    onDataDisplayAction: props.onDataDisplayAction,
  };
  const workflowCallbacks = useMemo<WorkflowConfigurationWidgetCallbacks>(
    () => ({
      onRequestConnection: (request) =>
        workflowCallbacksRef.current.onRequestConnection?.(request),
      onRefreshField: (request) =>
        workflowCallbacksRef.current.onRefreshField?.(request),
      onCopyField: (request) => workflowCallbacksRef.current.onCopyField?.(request),
      onDataDisplayAction: (request) =>
        workflowCallbacksRef.current.onDataDisplayAction?.(request),
    }),
    [],
  );
  const builtInWidgets = useMemo(
    () => createWorkflowConfigurationWidgetRegistry(workflowCallbacks),
    [workflowCallbacks],
  );
  const widgets = useMemo(
    () => ({ ...builtInWidgets, ...props.widgetRegistry }),
    [builtInWidgets, props.widgetRegistry],
  );
  const defaults = useMemo(
    () => createWorkflowNodeDefaultValue(props.node, formOptions),
    [formOptions, props.node],
  );
  const inputTypes = panelInputTypes(props.node);
  const outputTypes = panelOutputTypes(props.node);
  const activeFields = resolveWorkflowNodeFields(props.node, {
    buildConfig: props.buildConfig,
    fieldVisibility: props.fieldVisibility,
  });
  const isVisible = (field: WorkflowNodeFieldDefinition) =>
    isWorkflowNodeFieldVisible(field, undefined, props.fieldVisibility);
  const visibleCount = activeFields.filter(isVisible).length;
  const advancedCount = activeFields.filter((field) => field.advanced && isVisible(field)).length;
  const conditionalCount = activeFields.length - visibleCount;
  const manifestMetadata = props.node as WorkflowNodeDefinition & {
    manifestVersion?: number;
    owner?: string;
    role?: string;
    stableIdBinding?: string;
    internal?: boolean;
    ports?: {
      inputs?: readonly unknown[];
      outputs?: readonly unknown[];
    };
  };
  const portCount =
    (manifestMetadata.ports
      ? (manifestMetadata.ports.inputs?.length ?? 0) +
        (manifestMetadata.ports.outputs?.length ?? 0)
      : (inputTypes.length > 0 ? 1 : 0) + (outputTypes.length > 0 ? 1 : 0));
  const runtimeBinding =
    'runtimeBinding' in props.node && typeof props.node.runtimeBinding === 'string'
      ? props.node.runtimeBinding
      : undefined;
  const nodeVisual = workflowNodeVisual(props.node);
  const displayTitle = props.title ?? props.node.display_name;
  const displayDescription = props.description ?? props.node.description;
  const onTabKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    let nextTab: 'settings' | 'last-run' | undefined;
    if (event.key === 'Home') nextTab = 'settings';
    if (event.key === 'End') nextTab = 'last-run';
    if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
      nextTab = activeTab === 'settings' ? 'last-run' : 'settings';
    }
    if (!nextTab) return;
    event.preventDefault();
    setActiveTab(nextTab);
    (nextTab === 'settings' ? settingsTabRef : lastRunTabRef).current?.focus();
  };
  const renderNodeAccessory = useCallback(
    ({ node, valuePath, value, disabled }: FormNodeAccessoryContext) => (
      <WorkflowFieldAccessory
        node={node}
        valuePath={valuePath}
        value={value}
        disabled={disabled}
        locale={props.locale}
        callbacks={workflowCallbacks}
      />
    ),
    [props.locale, workflowCallbacks],
  );

  if (!compilation.ok || !compilation.plan || !compilation.document) {
    return (
      <section className="a3s-form-workflow-node-panel" role="alert">
        <strong>{copy.compileTitle}</strong>
        <p>
          {taskPresentation
            ? copy.compileHelp
            : (compilation.diagnostics[0]?.message ?? copy.compileHelp)}
        </p>
      </section>
    );
  }

  const compiledDocument = compilation.document;
  return (
    <section
      className={['a3s-form-workflow-node-panel', props.className].filter(Boolean).join(' ')}
      data-node-type={props.node.type}
      data-node-family={nodeVisual.family}
      data-node-tone={nodeVisual.tone}
      data-read-only={props.readOnly || undefined}
      data-manifest-version={manifestMetadata.manifestVersion}
      data-node-owner={manifestMetadata.owner}
      data-node-role={manifestMetadata.role}
      data-node-internal={String(manifestMetadata.internal === true)}
      data-node-official={String(props.node.official === true)}
      data-node-tool-mode={String(props.node.tool_mode === true)}
      data-stable-id-binding={manifestMetadata.stableIdBinding}
      data-field-count={activeFields.length}
      data-visible-field-count={visibleCount}
      data-advanced-field-count={advancedCount}
      data-port-count={portCount}
      aria-label={`${displayTitle} ${copy.panelSections}`}
    >
      <header className="a3s-form-workflow-node-panel-header">
        <div className="a3s-form-workflow-node-identity">
          <span
            className="a3s-form-workflow-node-icon"
            data-source-icon={props.node.icon || undefined}
            title={props.node.icon || props.node.categoryLabel}
          >
            <DesignerIcon name={nodeVisual.icon} size={18} />
          </span>
          <span>
            <span className="a3s-form-workflow-node-title-line">
              {taskPresentation && props.onTitleChange ? (
                <>
                  <h2 className="a3s-form-workflow-node-title-accessible">{displayTitle}</h2>
                  <input
                    className="input a3s-form-workflow-node-title-input"
                    aria-label={copy.nodeTitle}
                    value={displayTitle}
                    disabled={props.readOnly}
                    onChange={(event) => props.onTitleChange?.(event.target.value)}
                  />
                </>
              ) : (
                <h2>{displayTitle}</h2>
              )}
              {props.node.beta && (
                <span className="badge" data-variant="secondary">
                  {props.locale?.toLocaleLowerCase().startsWith('zh') ? '测试版' : 'Beta'}
                </span>
              )}
              {props.node.legacy && (
                <span className="badge" data-variant="outline">
                  {props.locale?.toLocaleLowerCase().startsWith('zh') ? '旧版' : 'Legacy'}
                </span>
              )}
            </span>
            {taskPresentation && props.onDescriptionChange ? (
              <input
                className="input a3s-form-workflow-node-description-input"
                aria-label={copy.nodeDescription}
                placeholder={copy.descriptionPlaceholder}
                value={displayDescription}
                disabled={props.readOnly}
                onChange={(event) => props.onDescriptionChange?.(event.target.value)}
              />
            ) : (
              <p>{displayDescription}</p>
            )}
          </span>
        </div>
        <div className="a3s-form-workflow-node-header-actions">
          {props.onRun && (
            <button
              type="button"
              className="btn"
              data-size="icon-sm"
              data-variant="ghost"
              aria-label={running ? copy.running : copy.run}
              disabled={props.readOnly || running}
              onClick={() => {
                setRunning(true);
                void Promise.resolve(props.onRun?.(props.value)).finally(() => setRunning(false));
              }}
            >
              <DesignerIcon name="play" size={14} />
            </button>
          )}
          {visibleCount > 0 && (
            <button
              type="button"
              className="btn"
              data-size="sm"
              data-variant="ghost"
              aria-label={resetPending ? copy.confirmReset : copy.reset}
              disabled={props.readOnly}
              onBlur={() => setResetPending(false)}
              onClick={() => {
                if (taskPresentation && !resetPending) {
                  setResetPending(true);
                  return;
                }
                const next = structuredClone(defaults);
                props.onChange(next);
                props.onReset?.(next);
                setResetPending(false);
              }}
            >
              <DesignerIcon name="undo" size={14} />
              <span>{resetPending ? copy.confirmReset : copy.reset}</span>
            </button>
          )}
          {props.showDocumentation !== false && props.node.documentation && (
            <a
              className="btn"
              data-size="sm"
              data-variant="ghost"
              aria-label={copy.reference}
              href={props.node.documentation}
              target="_blank"
              rel="noreferrer"
            >
              <DesignerIcon name="link" size={14} />
              <span>{copy.reference}</span>
            </a>
          )}
          {props.onClose && (
            <button
              type="button"
              className="btn"
              data-size="icon-sm"
              data-variant="ghost"
              aria-label={copy.close}
              onClick={props.onClose}
            >
              <DesignerIcon name="close" size={15} />
            </button>
          )}
        </div>
      </header>

      {taskPresentation && (
        <div className="a3s-form-workflow-node-tabs" role="tablist" aria-label={copy.panelSections}>
          <button
            type="button"
            ref={settingsTabRef}
            id={`${panelId}-settings-tab`}
            role="tab"
            aria-controls={`${panelId}-settings-panel`}
            aria-selected={activeTab === 'settings'}
            tabIndex={activeTab === 'settings' ? 0 : -1}
            onClick={() => setActiveTab('settings')}
            onKeyDown={onTabKeyDown}
          >
            {copy.settings}
          </button>
          <button
            type="button"
            ref={lastRunTabRef}
            id={`${panelId}-last-run-tab`}
            role="tab"
            aria-controls={`${panelId}-last-run-panel`}
            aria-selected={activeTab === 'last-run'}
            tabIndex={activeTab === 'last-run' ? 0 : -1}
            onClick={() => setActiveTab('last-run')}
            onKeyDown={onTabKeyDown}
          >
            {copy.lastRun}
          </button>
        </div>
      )}

      {!taskPresentation && (
        <div className="a3s-form-workflow-node-contract">
          <span className="badge" data-variant="outline">
            {props.node.categoryLabel}
          </span>
          {runtimeBinding && <code>{runtimeBinding}</code>}
          <span>
            {visibleCount} {copy.shown}
          </span>
          {advancedCount > 0 && (
            <span>
              {advancedCount} {copy.advanced}
            </span>
          )}
          {conditionalCount > 0 && (
            <span>
              {conditionalCount} {copy.conditional}
            </span>
          )}
        </div>
      )}

      {!taskPresentation && (inputTypes.length > 0 || outputTypes.length > 0) && (
        <div className="a3s-form-workflow-node-ports">
          <div>
            <span>{copy.accepts}</span>
            <div className="item-group">
              {(inputTypes.length > 0 ? inputTypes : ['No typed inputs']).map((type) => (
                <code className="badge" data-variant="outline" key={type}>
                  {type}
                </code>
              ))}
            </div>
          </div>
          <div>
            <span>{copy.returns}</span>
            <div className="item-group">
              {(outputTypes.length > 0 ? outputTypes : ['No typed outputs']).map((type) => (
                <code className="badge" data-variant="secondary" key={type}>
                  {type}
                </code>
              ))}
            </div>
          </div>
        </div>
      )}

      {(!taskPresentation || activeTab === 'settings') && (
        <div
          {...(taskPresentation
            ? {
                id: `${panelId}-settings-panel`,
                role: 'tabpanel',
                'aria-labelledby': `${panelId}-settings-tab`,
              }
            : {})}
          className="a3s-form-workflow-node-settings"
        >
          {visibleCount === 0 ? (
            <div className="a3s-form-workflow-node-settings-empty">
              <span aria-hidden="true">
                <DesignerIcon name="check-square" size={18} />
              </span>
              <strong>{copy.noSettingsTitle}</strong>
              <p>{copy.noSettingsHelp}</p>
            </div>
          ) : (
            <div className="a3s-form-workflow-node-form">
              <FormRenderer
                plan={compilation.plan}
                value={props.value}
                onChange={props.onChange}
                onAction={async (actionId, value) => {
                  if (actionId === 'apply') await props.onApply?.(value, compiledDocument);
                }}
                hostAdapter={props.hostAdapter}
                locale={props.locale}
                readOnly={props.readOnly}
                nodeRegistry={props.nodeRegistry}
                renderNodeAccessory={renderNodeAccessory}
                widgetRegistry={widgets}
              />
            </div>
          )}

          {taskPresentation && (
            <WorkflowNodeContractDetails
              buildConfig={props.buildConfig}
              fieldVisibility={props.fieldVisibility}
              locale={props.locale}
              node={props.node}
              value={props.value}
            />
          )}
        </div>
      )}

      {taskPresentation && activeTab === 'last-run' && (
        <div
          id={`${panelId}-last-run-panel`}
          role="tabpanel"
          aria-labelledby={`${panelId}-last-run-tab`}
          className="a3s-form-workflow-node-last-run"
        >
          {props.lastRun ?? (
            <div className="a3s-form-workflow-node-last-run-empty">
              <DesignerIcon name="play" size={18} />
              <p>{copy.emptyRun}</p>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
