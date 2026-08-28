const DURATION_UNIT_LABELS: Readonly<Record<string, readonly [string, string]>> = {
  Days: ['天', 'Days'],
  Hours: ['小时', 'Hours'],
  Milliseconds: ['毫秒', 'Milliseconds'],
  Minutes: ['分钟', 'Minutes'],
  Seconds: ['秒', 'Seconds'],
  Weeks: ['周', 'Weeks'],
};

export function isChineseWorkflowLocale(locale: string | undefined): boolean {
  return locale?.toLocaleLowerCase().startsWith('zh') === true;
}

export function workflowDurationUnitLabel(unit: string, locale: string | undefined): string {
  const labels = DURATION_UNIT_LABELS[unit];
  if (!labels) return unit;
  return labels[isChineseWorkflowLocale(locale) ? 0 : 1];
}

export function workflowWidgetCopy(locale: string | undefined) {
  const chinese = isChineseWorkflowLocale(locale);
  return chinese
    ? {
        live: '实时',
        toolInput: '工具输入',
        workflowInput: '工作流输入',
        runtimeConfigured: '由运行时提供',
        refreshField: (label: string) => `刷新${label}`,
        copyField: (label: string) => `复制${label}`,
        connectField: (label: string) => `连接${label}`,
        workflowInputLegend: (label: string) => `${label}工作流输入`,
        connect: '连接',
        connectionSet: '已连接',
        connectFromCanvas: '从工作流画布连接',
        anyCompatibleOutput: '任意兼容输出',
        change: '更换',
        choose: '选择',
        connectionLabel: (label: string, connected: boolean) =>
          `${connected ? '更换' : '选择'}${label}的连接`,
        expand: '展开',
        collapse: '收起',
        editorLabel: (label: string, expanded: boolean) =>
          `${expanded ? '收起' : '展开'}${label}编辑器`,
        empty: '空',
        lineCount: (count: number) => `${count} 行`,
        invalidJson: '请输入有效的 JSON 后再更新。',
        selectModel: '选择模型',
        chooseModel: (_modelType: string) => '选择模型',
        sortableOrder: (label: string) => `${label}顺序`,
        addSortableItem: (label: string) => `添加${label}`,
        addOperation: '添加一项…',
        noOperations: '尚未选择任何项目。',
        noAvailableOperations: '没有可添加的项目。',
        moveUp: (name: string) => `上移${name}`,
        moveDown: (name: string) => `下移${name}`,
        removeItem: (name: string) => `移除${name}`,
        selectedCount: (selected: number, limit: number) => `已选择 ${selected}/${limit} 项`,
        durationValue: (label: string) => `${label}数值`,
        durationUnit: (label: string) => `${label}单位`,
        decisionPlaceholder: '添加一个决策',
        addDecision: '添加',
        emptyDecision: '请输入要添加的决策。',
        duplicateDecision: '该决策已添加。',
        decisionLimit: (limit: number) => `最多添加 ${limit} 个决策。`,
        chooseFiles: '选择文件',
        chooseFile: '选择文件',
        noFileSelected: '尚未选择文件',
        selectedFiles: '已选择的文件',
        removeFile: (name: string) => `移除文件${name}`,
        clearFiles: '清除文件',
        invalidFileType: (names: string[], types: string[]) =>
          `不支持${names.join('、')}。允许类型：${types.join(' · ')}。`,
        mcpServer: 'MCP 服务',
        configurationReady: '配置已就绪',
        notConfigured: '尚未配置',
        noData: '暂无数据。',
        minimum: '最小值',
        maximum: '最大值',
        step: '步长',
      }
    : {
        live: 'Live',
        toolInput: 'Tool input',
        workflowInput: 'Workflow input',
        runtimeConfigured: 'Runtime configured',
        refreshField: (label: string) => `Refresh ${label}`,
        copyField: (label: string) => `Copy ${label}`,
        connectField: (label: string) => `Connect ${label}`,
        workflowInputLegend: (label: string) => `${label} workflow input`,
        connect: 'Connect',
        connectionSet: 'Connection set',
        connectFromCanvas: 'Connect from the workflow canvas',
        anyCompatibleOutput: 'Any compatible output',
        change: 'Change',
        choose: 'Choose',
        connectionLabel: (label: string, connected: boolean) =>
          `${connected ? 'Change' : 'Choose'} ${label} connection`,
        expand: 'Expand',
        collapse: 'Collapse',
        editorLabel: (label: string, expanded: boolean) =>
          `${expanded ? 'Collapse' : 'Expand'} ${label} editor`,
        empty: 'Empty',
        lineCount: (count: number) => `${count} ${count === 1 ? 'line' : 'lines'}`,
        invalidJson: 'Enter valid JSON to update this value.',
        selectModel: 'Select a model',
        chooseModel: (modelType: string) => `Choose a ${modelType} model`,
        sortableOrder: (label: string) => `${label} order`,
        addSortableItem: (label: string) => `Add ${label}`,
        addOperation: 'Add an operation…',
        noOperations: 'No operations selected.',
        noAvailableOperations: 'No more options are available.',
        moveUp: (name: string) => `Move ${name} up`,
        moveDown: (name: string) => `Move ${name} down`,
        removeItem: (name: string) => `Remove ${name}`,
        selectedCount: (selected: number, limit: number) => `${selected} of ${limit} selected`,
        durationValue: (label: string) => `${label} value`,
        durationUnit: (label: string) => `${label} unit`,
        decisionPlaceholder: 'Add a decision',
        addDecision: 'Add',
        emptyDecision: 'Enter a decision to add.',
        duplicateDecision: 'That decision is already added.',
        decisionLimit: (limit: number) => `You can add up to ${limit} decisions.`,
        chooseFiles: 'Choose files',
        chooseFile: 'Choose a file',
        noFileSelected: 'No file selected',
        selectedFiles: 'Selected files',
        removeFile: (name: string) => `Remove file ${name}`,
        clearFiles: 'Clear files',
        invalidFileType: (names: string[], types: string[]) =>
          `${names.join(', ')} is not supported. Allowed types: ${types.join(', ')}.`,
        mcpServer: 'MCP server',
        configurationReady: 'Configuration ready',
        notConfigured: 'Not configured',
        noData: 'No data available.',
        minimum: 'Min',
        maximum: 'Max',
        step: 'Step',
      };
}
