import type { FlowWebsiteLocale } from './flow-node-catalog';

export type WorkflowPlaygroundCopy = {
  title: string;
  intro: string;
  version: (version: string) => string;
  nodes: (count: number) => string;
  edges: (count: number) => string;
  ready: string;
  needsAttention: string;
  catalog: string;
  searchLabel: string;
  searchPlaceholder: string;
  noSearchResults: string;
  addNode: (name: string) => string;
  dragNode: (name: string) => string;
  canvas: string;
  canvasHelp: string;
  dropHelp: string;
  fit: string;
  reset: string;
  validate: string;
  deleteSelection: string;
  emptyCanvas: string;
  emptyCanvasDetail: string;
  settings: string;
  validation: string;
  document: string;
  noSelection: string;
  noSelectionDetail: string;
  selectNode: string;
  connectedOutputs: (count: number) => string;
  browserOnly: string;
  validTitle: string;
  validDetail: string;
  invalidTitle: string;
  invalidDetail: string;
  configurationTitle: string;
  configurationValid: string;
  configurationInvalid: (count: number) => string;
  planTitle: string;
  graphIssues: string;
  configurationIssues: string;
  topLevel: string;
  scope: string;
  copyDocument: string;
  copied: string;
  copyFailed: string;
  resetDone: string;
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
  minimap: string;
  zoomControls: string;
};

export const workflowPlaygroundCopy: Readonly<
  Record<FlowWebsiteLocale, WorkflowPlaygroundCopy>
> = {
  zh: {
    title: '在浏览器里把一张工作流图接起来',
    intro:
      '节点、端口、默认值、配置表单和编译结果都来自当前版本的 Flow 契约。这里生成的文档可以继续交给 CLI 校验，也可以交给宿主编辑器保存。',
    version: (version) => `契约 ${version}`,
    nodes: (count) => `${count} 个节点`,
    edges: (count) => `${count} 条连线`,
    ready: '可以编译',
    needsAttention: '需要处理',
    catalog: '节点库',
    searchLabel: '搜索节点',
    searchPlaceholder: '按名称或类型搜索',
    noSearchResults: '没有找到匹配的节点。',
    addNode: (name) => `添加${name}`,
    dragNode: (name) => `拖动${name}到画布，或点击直接添加`,
    canvas: '工作流画布',
    canvasHelp: '拖动节点调整位置，从输出端口拉线到输入端口。',
    dropHelp: '松开后添加节点',
    fit: '适应画布',
    reset: '恢复示例',
    validate: '检查工作流',
    deleteSelection: '删除选中项',
    emptyCanvas: '画布还是空的',
    emptyCanvasDetail: '从左侧点击一个节点，或把节点拖到这里。',
    settings: '配置',
    validation: '校验',
    document: 'DSL',
    noSelection: '还没有选中节点',
    noSelectionDetail: '选择画布中的节点后，这里会打开它的真实配置表单。',
    selectNode: '在画布中选择节点',
    connectedOutputs: (count) => `已连接 ${count} 个输出端口`,
    browserOnly:
      '这个页面负责编辑和编译，不会在浏览器里执行外部任务。实际运行由接入 Flow 的宿主和 Worker 完成。',
    validTitle: '图结构可以编译',
    validDetail:
      '拓扑顺序已经生成。保存或执行前仍应由宿主确认凭据、权限与任务实现。',
    invalidTitle: '还有问题需要处理',
    invalidDetail: '按下面的路径修改图结构或节点配置，再重新检查。',
    configurationTitle: '节点配置',
    configurationValid: '所有可检查的节点配置均有效。',
    configurationInvalid: (count) => `发现 ${count} 个配置问题。`,
    planTitle: '编译顺序',
    graphIssues: '图结构问题',
    configurationIssues: '节点配置问题',
    topLevel: '顶层',
    scope: '子画布',
    copyDocument: '复制 DSL',
    copied: 'DSL 已复制到剪贴板。',
    copyFailed: '无法写入剪贴板，请从代码区手动复制。',
    resetDone: '已恢复示例工作流。',
    nodeAdded: (name) => `已添加${name}。`,
    containerAdded: (name) => `已添加${name}，并创建内部起始节点和示例任务。`,
    selectionDeleted: '已删除选中的节点或连线。',
    nothingSelected: '请先选择要删除的节点或连线。',
    nodeUpdated: '节点配置已更新。',
    connectionCreated: '连线已创建。',
    connectionRequest: (path) =>
      `请从上游数据端口拉线到当前节点。目标字段 ${path}`,
    connectionRejected: {
      missing_endpoint: '连线必须同时指定起点和终点。',
      missing_handle: '请从明确的输出端口连接到输入端口。',
      missing_node: '连线引用的节点已经不存在。',
      unknown_port: '这个端口不属于当前节点。',
      unavailable_port: '当前配置下，这个输出端口不可用。',
      incompatible_port: '两个端口承载的数据类型不兼容。',
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
    minimap: '工作流缩略图',
    zoomControls: '画布缩放控制',
  },
  en: {
    title: 'Connect a workflow graph in the browser',
    intro:
      'Nodes, ports, defaults, configuration forms, and compiler output all come from the current Flow contract. Continue with the CLI for validation or hand the document to a host editor for persistence.',
    version: (version) => `Contract ${version}`,
    nodes: (count) => `${count} nodes`,
    edges: (count) => `${count} edges`,
    ready: 'Compiles',
    needsAttention: 'Needs attention',
    catalog: 'Node library',
    searchLabel: 'Search nodes',
    searchPlaceholder: 'Search by name or type',
    noSearchResults: 'No nodes match this search.',
    addNode: (name) => `Add ${name}`,
    dragNode: (name) => `Drag ${name} to the canvas or click to add it`,
    canvas: 'Workflow canvas',
    canvasHelp: 'Move nodes, then connect an output port to an input port.',
    dropHelp: 'Release to add the node',
    fit: 'Fit canvas',
    reset: 'Reset sample',
    validate: 'Check workflow',
    deleteSelection: 'Delete selection',
    emptyCanvas: 'The canvas is empty',
    emptyCanvasDetail:
      'Click a node in the library or drag one onto the canvas.',
    settings: 'Configure',
    validation: 'Validate',
    document: 'DSL',
    noSelection: 'No node selected',
    noSelectionDetail:
      'Select a canvas node to open its production configuration form here.',
    selectNode: 'Select a node on the canvas',
    connectedOutputs: (count) => `${count} output ports connected`,
    browserOnly:
      'This page edits and compiles the document. External tasks do not run in the browser; an integrated Flow host and its workers perform execution.',
    validTitle: 'The graph compiles',
    validDetail:
      'A deterministic topology is available. The host still confirms credentials, authorization, and task implementations before execution.',
    invalidTitle: 'Some issues still need attention',
    invalidDetail:
      'Update the graph or node configuration at the listed paths, then check again.',
    configurationTitle: 'Node configuration',
    configurationValid:
      'Every configuration that can be checked here is valid.',
    configurationInvalid: (count) => `${count} configuration issues found.`,
    planTitle: 'Compilation order',
    graphIssues: 'Graph issues',
    configurationIssues: 'Node configuration issues',
    topLevel: 'Top level',
    scope: 'Child canvas',
    copyDocument: 'Copy DSL',
    copied: 'DSL copied to the clipboard.',
    copyFailed:
      'Clipboard access failed. Copy the document from the code area.',
    resetDone: 'The sample workflow has been restored.',
    nodeAdded: (name) => `${name} added.`,
    containerAdded: (name) =>
      `${name} added with its internal start node and an example task.`,
    selectionDeleted: 'The selected nodes or edges were deleted.',
    nothingSelected: 'Select a node or edge before deleting.',
    nodeUpdated: 'Node configuration updated.',
    connectionCreated: 'Connection created.',
    connectionRequest: (path) =>
      `Connect an upstream data port to the current node. Target field: ${path}`,
    connectionRejected: {
      missing_endpoint: 'A connection needs both a source and a target.',
      missing_handle: 'Connect a specific output port to an input port.',
      missing_node: 'A referenced node no longer exists.',
      unknown_port: 'This port does not belong to the current node.',
      unavailable_port:
        'This output port is unavailable with the current configuration.',
      incompatible_port: 'The two ports carry incompatible data types.',
      cross_scope:
        'Top-level and child-canvas nodes cannot be connected directly.',
      self_edge: 'A node cannot connect to itself.',
      duplicate_edge: 'This connection already exists.',
      occupied_input: 'This input port already has a connection.',
      cycle: 'This connection would create a cycle in the current scope.',
    },
    sourceHandle: (name) => `${name} output port`,
    targetHandle: (name) => `${name} input port`,
    childCanvas: 'Child canvas',
    internalNode: 'Internal node',
    minimap: 'Workflow minimap',
    zoomControls: 'Canvas zoom controls',
  },
};
