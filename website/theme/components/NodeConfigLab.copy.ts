import type { LocalText, NodeConfigLocale } from './NodeConfigLab.types';

export const nodeConfigCopy = {
  zh: {
    addItem: '添加成员',
    catalogLabel: '节点类型',
    copied: '已复制',
    copy: '复制 JSON',
    copyFailed: '复制失败',
    coverage: '14 个命令 · 2 个容器',
    formLabel: '节点配置',
    graphPreview: '图结构预览',
    item: '成员',
    preview: '命令预览',
    removeItem: '移除',
    required: '必填',
    requiredHint: '标有 * 的字段必须填写。切换节点时会保留本页草稿。',
    reset: '恢复示例',
    valid: '配置有效',
    invalid: '请修正配置',
    versionNote: 'A3S Flow 1.0 配置契约',
  },
  en: {
    addItem: 'Add member',
    catalogLabel: 'Node type',
    copied: 'Copied',
    copy: 'Copy JSON',
    copyFailed: 'Copy failed',
    coverage: '14 commands · 2 containers',
    formLabel: 'Node configuration',
    graphPreview: 'Graph structure preview',
    item: 'Member',
    preview: 'Command preview',
    removeItem: 'Remove',
    required: 'required',
    requiredHint:
      'Fields marked * are required. Drafts remain when you switch nodes.',
    reset: 'Reset example',
    valid: 'Configuration is valid',
    invalid: 'Fix the configuration',
    versionNote: 'A3S Flow 1.0 configuration contract',
  },
};

export function localized(value: LocalText, locale: NodeConfigLocale) {
  return value[locale];
}
