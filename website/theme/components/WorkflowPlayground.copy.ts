import type { FlowWebsiteLocale } from './flow-node-catalog';
import type {
  PlaygroundEdgeColor,
  PlaygroundEdgeRouting,
} from './WorkflowPlayground.model';

export type WorkflowPlaygroundCopy = {
  pageTitle: string;
  workflowName: string;
  localDraft: string;
  saved: string;
  saving: string;
  backHome: string;
  backToExamples: string;
  language: string;
  version: string;
  validate: string;
  run: string;
  stop: string;
  moreActions: string;
  reset: string;
  exportGraph: string;
  openDocument: string;
  addNode: string;
  addNote: string;
  addComment: string;
  addNamedNode: (name: string) => string;
  selectMode: string;
  panMode: string;
  commentHelp: string;
  noteLabel: string;
  commentLabel: string;
  notePlaceholder: string;
  commentPlaceholder: string;
  deleteAnnotation: string;
  annotationAdded: Record<'note' | 'comment', string>;
  arrangeNodes: string;
  nodesArranged: string;
  edgeRouting: string;
  curvedEdges: string;
  orthogonalEdges: string;
  edgeRoutingToggle: Record<PlaygroundEdgeRouting, string>;
  edgeRoutingChanged: Record<PlaygroundEdgeRouting, string>;
  edgeColor: string;
  edgeColorNames: Record<PlaygroundEdgeColor, string>;
  edgeColorChanged: (name: string) => string;
  showMinimap: string;
  hideMinimap: string;
  minimapPaused: string;
  canvasTools: string;
  undo: string;
  redo: string;
  history: string;
  variables: string;
  trace: string;
  close: string;
  nodeLibrary: string;
  nodeLibraryDescription: string;
  builtInNodes: string;
  customNodes: string;
  capabilityReady: string;
  capabilityHandler: string;
  noCustomNodes: string;
  searchNodes: string;
  noNodes: string;
  canvasLabel: string;
  dropHelp: string;
  fitView: string;
  minimap: string;
  zoomControls: string;
  noSelection: string;
  noSelectionDetail: string;
  settings: string;
  validation: string;
  document: string;
  runStep: string;
  duplicate: string;
  delete: string;
  selectedNode: string;
  connectedOutputs: (count: number) => string;
  validationReady: string;
  validationNeedsWork: (count: number) => string;
  validTitle: string;
  validDetail: string;
  invalidTitle: string;
  invalidDetail: string;
  configurationTitle: string;
  configurationValid: string;
  configurationInvalid: (count: number) => string;
  planTitle: string;
  graphIssues: string;
  topLevel: string;
  scope: string;
  copyDocument: string;
  copied: string;
  copyFailed: string;
  resetDone: string;
  graphExported: string;
  nodeAdded: (name: string) => string;
  containerAdded: (name: string) => string;
  selectionDeleted: string;
  nothingSelected: string;
  nodeUpdated: string;
  connectionCreated: string;
  connectionRequest: (path: string) => string;
  connectionRejected: Record<string, string>;
  sourceHandle: (name: string) => string;
  targetHandle: (name: string) => string;
  childCanvas: string;
  internalNode: string;
  localRun: string;
  runComplete: string;
  runStopped: string;
  noTrace: string;
  noHistory: string;
  runHistory: string;
  cachedVariables: string;
  documentPreview: string;
  readOnly: string;
  lineCount: (count: number) => string;
  characterCount: (count: number) => string;
};

export const workflowPlaygroundCopy: Readonly<
  Record<FlowWebsiteLocale, WorkflowPlaygroundCopy>
> = {
  zh: {
    pageTitle: 'A3S Flow 工作流设计器',
    workflowName: '跨境高价值订单履约',
    localDraft: '本地草稿',
    saved: '已保存到本地',
    saving: '正在保存',
    backHome: '返回 A3S Flow 文档',
    backToExamples: '返回工作流示例列表',
    language: '切换到英文',
    version: '版本',
    validate: '校验',
    run: '试运行',
    stop: '停止',
    moreActions: '更多操作',
    reset: '恢复示例',
    exportGraph: '导出工作流',
    openDocument: '查看 DSL',
    addNode: '添加节点',
    addNote: '添加便笺',
    addComment: '添加批注',
    addNamedNode: (name) => `添加${name}`,
    selectMode: '选择并移动节点',
    panMode: '平移画布',
    commentHelp: '在画布空白处单击，放下一条批注',
    noteLabel: '便笺',
    commentLabel: '批注',
    notePlaceholder: '记下这段流程的设计说明…',
    commentPlaceholder: '写下需要讨论或修改的地方…',
    deleteAnnotation: '删除',
    annotationAdded: {
      note: '便笺已添加。',
      comment: '批注已添加。',
    },
    arrangeNodes: '整理节点',
    nodesArranged: '节点已按执行顺序排列。',
    edgeRouting: '连线路由',
    curvedEdges: '曲线',
    orthogonalEdges: '折线',
    edgeRoutingToggle: {
      curve: '切换为折线',
      orthogonal: '切换为曲线',
    },
    edgeRoutingChanged: {
      curve: '连线已切换为曲线。',
      orthogonal: '连线已切换为折线。',
    },
    edgeColor: '连线颜色',
    edgeColorNames: {
      blue: '蓝色',
      teal: '青绿色',
      violet: '紫色',
      amber: '琥珀色',
    },
    edgeColorChanged: (name) => `连线颜色已切换为${name}。`,
    showMinimap: '显示缩略图',
    hideMinimap: '隐藏缩略图',
    minimapPaused: '大图已暂停缩略图以保持滚动流畅',
    canvasTools: '画布工具',
    undo: '撤销',
    redo: '重做',
    history: '运行历史',
    variables: '变量检查',
    trace: '运行轨迹',
    close: '关闭',
    nodeLibrary: '节点库',
    nodeLibraryDescription: '从内置清单或已授权的自定义节点中选择。',
    builtInNodes: '内置节点',
    customNodes: '自定义节点',
    capabilityReady: '能力已绑定',
    capabilityHandler: '处理器',
    noCustomNodes: '当前没有已注册的自定义节点。',
    searchNodes: '搜索节点名称、说明或类型',
    noNodes: '没有匹配的节点。',
    canvasLabel: '交互式工作流画布',
    dropHelp: '松开后把节点放到这里',
    fitView: '适应画布',
    minimap: '工作流缩略图',
    zoomControls: '画布缩放控制',
    noSelection: '还没有选中节点',
    noSelectionDetail: '选择画布中的节点后，这里会打开它的配置表单。',
    settings: '配置',
    validation: '校验结果',
    document: '工作流 DSL',
    runStep: '试运行当前节点',
    duplicate: '复制节点',
    delete: '删除节点',
    selectedNode: '当前节点',
    connectedOutputs: (count) => `已连接 ${count} 个输出端口`,
    validationReady: '图结构有效',
    validationNeedsWork: (count) => `${count} 个问题待处理`,
    validTitle: '工作流可以编译',
    validDetail: '节点配置和图结构已经通过当前版本的契约检查。',
    invalidTitle: '工作流还不能编译',
    invalidDetail: '按路径修改节点配置或连线后再校验。',
    configurationTitle: '节点配置',
    configurationValid: '所有可检查的节点配置均有效。',
    configurationInvalid: (count) => `发现 ${count} 个配置问题。`,
    planTitle: '编译顺序',
    graphIssues: '图结构问题',
    topLevel: '顶层画布',
    scope: '子画布',
    copyDocument: '复制 DSL',
    copied: 'DSL 已复制。',
    copyFailed: '浏览器没有允许写入剪贴板，请从代码区手动复制。',
    resetDone: '已恢复示例工作流。',
    graphExported: '工作流文件已导出。',
    nodeAdded: (name) => `已添加${name}。`,
    containerAdded: (name) => `已添加${name}及其内部起始节点。`,
    selectionDeleted: '已删除选中的节点或连线。',
    nothingSelected: '请先选择节点或连线。',
    nodeUpdated: '节点配置已更新。',
    connectionCreated: '连线已创建。',
    connectionRequest: (path) => `请连接上游数据到字段 ${path}。`,
    connectionRejected: {
      missing_endpoint: '连线必须同时指定起点和终点。',
      missing_handle: '请从明确的输出端口连接到输入端口。',
      missing_node: '连线引用的节点已经不存在。',
      unknown_port: '这个端口不属于当前节点。',
      unavailable_port: '当前配置下，这个输出端口不可用。',
      incompatible_port: '两个端口的数据类型不兼容。',
      cross_scope: '顶层画布和子画布之间不能直接拉线。',
      self_edge: '节点不能连接到自己。',
      duplicate_edge: '这条连线已经存在。',
      occupied_input: '这个输入端口已经有一条连线。',
      cycle: '这条连线会在当前作用域中形成环路。',
    },
    sourceHandle: (name) => `${name}输出端口`,
    targetHandle: (name) => `${name}输入端口`,
    childCanvas: '子画布',
    internalNode: '内部节点',
    localRun: '试运行只展示本地执行顺序，不会调用外部任务。',
    runComplete: '试运行完成。',
    runStopped: '试运行已停止。',
    noTrace: '还没有运行记录。',
    noHistory: '完成一次试运行后，这里会保留最近记录。',
    runHistory: '运行历史',
    cachedVariables: '变量检查',
    documentPreview: '当前 DSL',
    readOnly: '只读预览',
    lineCount: (count) => `${count} 行`,
    characterCount: (count) => `${count} 个字符`,
  },
  en: {
    pageTitle: 'A3S Flow workflow designer',
    workflowName: 'Cross-border high-value order fulfillment',
    localDraft: 'Local draft',
    saved: 'Saved locally',
    saving: 'Saving',
    backHome: 'Back to A3S Flow documentation',
    backToExamples: 'Back to workflow examples',
    language: 'Switch to Chinese',
    version: 'Version',
    validate: 'Validate',
    run: 'Test run',
    stop: 'Stop',
    moreActions: 'More actions',
    reset: 'Reset sample',
    exportGraph: 'Export workflow',
    openDocument: 'View DSL',
    addNode: 'Add node',
    addNote: 'Add note',
    addComment: 'Add comment',
    addNamedNode: (name) => `Add ${name}`,
    selectMode: 'Select and move nodes',
    panMode: 'Pan canvas',
    commentHelp: 'Click an empty area of the canvas to place a comment',
    noteLabel: 'Note',
    commentLabel: 'Comment',
    notePlaceholder: 'Record a design note for this part of the workflow…',
    commentPlaceholder: 'Write down what needs discussion or revision…',
    deleteAnnotation: 'Delete',
    annotationAdded: {
      note: 'Note added.',
      comment: 'Comment added.',
    },
    arrangeNodes: 'Tidy nodes',
    nodesArranged: 'Nodes arranged in execution order.',
    edgeRouting: 'Connection routing',
    curvedEdges: 'Curved connections',
    orthogonalEdges: 'Orthogonal connections',
    edgeRoutingToggle: {
      curve: 'Switch to orthogonal connections',
      orthogonal: 'Switch to curved connections',
    },
    edgeRoutingChanged: {
      curve: 'Connections now use curves.',
      orthogonal: 'Connections now use orthogonal lines.',
    },
    edgeColor: 'Connection color',
    edgeColorNames: {
      blue: 'Blue',
      teal: 'Teal',
      violet: 'Violet',
      amber: 'Amber',
    },
    edgeColorChanged: (name) => `Connection color changed to ${name}.`,
    showMinimap: 'Show minimap',
    hideMinimap: 'Hide minimap',
    minimapPaused: 'Minimap paused for large graphs',
    canvasTools: 'Canvas tools',
    undo: 'Undo',
    redo: 'Redo',
    history: 'Run history',
    variables: 'Variable inspect',
    trace: 'Run trace',
    close: 'Close',
    nodeLibrary: 'Node library',
    nodeLibraryDescription:
      'Choose from the built-in catalog or authorized custom nodes.',
    builtInNodes: 'Built-in nodes',
    customNodes: 'Custom nodes',
    capabilityReady: 'Capability bound',
    capabilityHandler: 'Handler',
    noCustomNodes: 'No custom nodes are registered.',
    searchNodes: 'Search by node name, description, or type',
    noNodes: 'No nodes match this search.',
    canvasLabel: 'Interactive workflow canvas',
    dropHelp: 'Release to place the node here',
    fitView: 'Fit canvas',
    minimap: 'Workflow minimap',
    zoomControls: 'Canvas zoom controls',
    noSelection: 'No node selected',
    noSelectionDetail:
      'Select a canvas node to open its configuration form here.',
    settings: 'Settings',
    validation: 'Validation',
    document: 'Workflow DSL',
    runStep: 'Test this node',
    duplicate: 'Duplicate node',
    delete: 'Delete node',
    selectedNode: 'Selected node',
    connectedOutputs: (count) => `${count} output ports connected`,
    validationReady: 'Graph is valid',
    validationNeedsWork: (count) => `${count} issues to resolve`,
    validTitle: 'The workflow compiles',
    validDetail:
      'Node settings and graph structure satisfy the current contract.',
    invalidTitle: 'The workflow cannot compile yet',
    invalidDetail:
      'Update the listed settings or connections and validate again.',
    configurationTitle: 'Node configuration',
    configurationValid: 'Every configuration checked here is valid.',
    configurationInvalid: (count) => `${count} configuration issues found.`,
    planTitle: 'Compilation order',
    graphIssues: 'Graph issues',
    topLevel: 'Top-level canvas',
    scope: 'Child canvas',
    copyDocument: 'Copy DSL',
    copied: 'DSL copied.',
    copyFailed:
      'Clipboard access was denied. Copy the document from the code area.',
    resetDone: 'The sample workflow has been restored.',
    graphExported: 'The workflow file has been exported.',
    nodeAdded: (name) => `${name} added.`,
    containerAdded: (name) => `${name} and its internal start node added.`,
    selectionDeleted: 'The selected node or connection was deleted.',
    nothingSelected: 'Select a node or connection first.',
    nodeUpdated: 'Node settings updated.',
    connectionCreated: 'Connection created.',
    connectionRequest: (path) => `Connect upstream data to ${path}.`,
    connectionRejected: {
      missing_endpoint: 'A connection needs both a source and a target.',
      missing_handle: 'Connect a specific output port to an input port.',
      missing_node: 'A referenced node no longer exists.',
      unknown_port: 'This port does not belong to the current node.',
      unavailable_port:
        'This output port is unavailable with the current settings.',
      incompatible_port: 'The two ports carry incompatible data types.',
      cross_scope: 'Top-level and child-canvas nodes cannot connect directly.',
      self_edge: 'A node cannot connect to itself.',
      duplicate_edge: 'This connection already exists.',
      occupied_input: 'This input port already has a connection.',
      cycle: 'This connection would create a cycle in the current scope.',
    },
    sourceHandle: (name) => `${name} output port`,
    targetHandle: (name) => `${name} input port`,
    childCanvas: 'Child canvas',
    internalNode: 'Internal node',
    localRun:
      'The test run only previews local execution order and never calls external tasks.',
    runComplete: 'Test run complete.',
    runStopped: 'Test run stopped.',
    noTrace: 'No run trace yet.',
    noHistory: 'Recent records appear here after a test run.',
    runHistory: 'Run history',
    cachedVariables: 'Variable inspect',
    documentPreview: 'Current DSL',
    readOnly: 'Read-only preview',
    lineCount: (count) => `${count} ${count === 1 ? 'line' : 'lines'}`,
    characterCount: (count) =>
      `${count} ${count === 1 ? 'character' : 'characters'}`,
  },
};
