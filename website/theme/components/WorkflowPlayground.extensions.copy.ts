import type { FlowWebsiteLocale } from './flow-node-catalog';

export type WorkflowPlaygroundExtensionCopy = {
  eyebrow: string;
  title: string;
  description: string;
  close: string;
  cli: string;
  skill: string;
  copilot: string;
  cliTitle: string;
  cliDescription: string;
  cliHint: string;
  cliValidate: string;
  cliCompile: string;
  cliDigest: string;
  cliInput: string;
  copyDsl: string;
  copied: string;
  copyFailed: string;
  skillTitle: string;
  skillDescription: string;
  skillPath: string;
  skillPrompt: string;
  contextTitle: string;
  contextNodes: (count: number) => string;
  contextEdges: (count: number) => string;
  contextSelection: string;
  noSelection: string;
  canvasSelection: string;
  nodeSelection: (id: string) => string;
  edgeSelection: (id: string) => string;
  annotationSelection: (id: string) => string;
  copilotTitle: string;
  copilotDescription: string;
  copilotPlaceholder: string;
  copilotSend: string;
  copilotCopyContext: string;
  copilotUnavailable: string;
  copilotSent: string;
  copilotFailed: string;
  copilotQuickReview: string;
  copilotQuickExplain: string;
  copilotQuickImprove: string;
  hostProvided: string;
  localPreview: string;
};

export const workflowPlaygroundExtensionCopy: Readonly<
  Record<FlowWebsiteLocale, WorkflowPlaygroundExtensionCopy>
> = {
  zh: {
    eyebrow: 'DESIGNER EXTENSIONS',
    title: '设计器扩展',
    description: '把当前 DSL 和选择上下文交给 CLI、Skill 或宿主 Copilot。',
    close: '关闭设计器扩展',
    cli: 'CLI',
    skill: 'Skill',
    copilot: 'Copilot',
    cliTitle: '从终端检查当前工作流',
    cliDescription:
      '将下方 DSL 保存为 workflow.json，或通过标准输入交给本地 a3s-flow。',
    cliHint: '这些命令读取同一份 DSL，不会修改画布。',
    cliValidate: '校验文档、节点配置与连线',
    cliCompile: '生成确定性执行计划',
    cliDigest: '生成语义摘要',
    cliInput: '标准输入',
    copyDsl: '复制完整 DSL',
    copied: '完整 DSL 已复制。',
    copyFailed: '剪贴板不可用，请从下方代码区复制。',
    skillTitle: '给编码 Agent 的上下文',
    skillDescription:
      'a3s-flow Skill 会先查询真实 manifest，再创建、连接、校验和审阅工作流。',
    skillPath: 'Skill 入口',
    skillPrompt: '建议指令',
    contextTitle: '当前上下文',
    contextNodes: (count) => `${count} 个节点`,
    contextEdges: (count) => `${count} 条连线`,
    contextSelection: '当前选择',
    noSelection: '未选择对象',
    canvasSelection: '画布',
    nodeSelection: (id) => `节点 · ${id}`,
    edgeSelection: (id) => `连线 · ${id}`,
    annotationSelection: (id) => `批注 · ${id}`,
    copilotTitle: '让 Copilot 理解这张图',
    copilotDescription:
      '请求会携带完整 DSL、选中对象及其相邻连线；宿主可以接入真实 Agent。',
    copilotPlaceholder:
      '例如：检查当前选择是否存在配置或连线问题，并给出修改建议。',
    copilotSend: '发送到宿主 Copilot',
    copilotCopyContext: '复制请求上下文',
    copilotUnavailable:
      '当前页面没有连接宿主 Copilot，仍可复制完整请求上下文。',
    copilotSent: '请求已交给宿主 Copilot。',
    copilotFailed: '宿主 Copilot 暂时不可用，请重试或复制请求上下文。',
    copilotQuickReview: '审查当前选择',
    copilotQuickExplain: '解释执行路径',
    copilotQuickImprove: '提出可靠性改进',
    hostProvided: '宿主提供',
    localPreview: '本地预览',
  },
  en: {
    eyebrow: 'DESIGNER EXTENSIONS',
    title: 'Designer extensions',
    description:
      'Give CLI, Skill, or a host Copilot the current DSL and selection context.',
    close: 'Close designer extensions',
    cli: 'CLI',
    skill: 'Skill',
    copilot: 'Copilot',
    cliTitle: 'Inspect this workflow from a terminal',
    cliDescription:
      'Save the DSL as workflow.json, or pipe it to the local a3s-flow CLI.',
    cliHint: 'These commands read the same DSL and never mutate the canvas.',
    cliValidate: 'Validate document, node settings, and connections',
    cliCompile: 'Build a deterministic execution plan',
    cliDigest: 'Create semantic document and graph digests',
    cliInput: 'stdin',
    copyDsl: 'Copy full DSL',
    copied: 'Full DSL copied.',
    copyFailed: 'Clipboard access failed. Copy it from the code block below.',
    skillTitle: 'Context for a coding Agent',
    skillDescription:
      'The a3s-flow Skill queries real manifests before creating, connecting, validating, or reviewing a workflow.',
    skillPath: 'Skill entry',
    skillPrompt: 'Suggested instruction',
    contextTitle: 'Current context',
    contextNodes: (count) => `${count} node${count === 1 ? '' : 's'}`,
    contextEdges: (count) => `${count} edge${count === 1 ? '' : 's'}`,
    contextSelection: 'Selection',
    noSelection: 'Nothing selected',
    canvasSelection: 'Canvas',
    nodeSelection: (id) => `Node · ${id}`,
    edgeSelection: (id) => `Edge · ${id}`,
    annotationSelection: (id) => `Annotation · ${id}`,
    copilotTitle: 'Help Copilot understand this graph',
    copilotDescription:
      'Requests include the complete DSL, selected object, and adjacent connections; a host can provide the actual Agent.',
    copilotPlaceholder:
      'For example: review the selected object for configuration or connection issues and suggest improvements.',
    copilotSend: 'Send to host Copilot',
    copilotCopyContext: 'Copy request context',
    copilotUnavailable:
      'No host Copilot is connected on this page. You can still copy the complete request context.',
    copilotSent: 'Request sent to the host Copilot.',
    copilotFailed:
      'The host Copilot is unavailable. Try again or copy the request context.',
    copilotQuickReview: 'Review selection',
    copilotQuickExplain: 'Explain execution path',
    copilotQuickImprove: 'Suggest reliability improvements',
    hostProvided: 'Host provided',
    localPreview: 'Local preview',
  },
};
