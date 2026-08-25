import {
  defineA3SFlowCustomDagNode,
  type A3SFlowCustomDagNodeRegistration,
  type A3SFlowDagPortDefinition,
  type WorkflowNodeFieldDefinition,
} from '@a3s-lab/flow-ui';
import type { FlowWebsiteLocale } from './flow-node-catalog';

type LocalizedText = Readonly<Record<FlowWebsiteLocale, string>>;

const text = (zh: string, en: string): LocalizedText => ({ zh, en });

function controlPort(id: string, label: string): A3SFlowDagPortDefinition {
  return { id, label, kind: 'control', types: ['FlowControl'] };
}

function dataPort(
  id: string,
  label: string,
  types: readonly string[],
): A3SFlowDagPortDefinition {
  return { id, label, kind: 'data', types };
}

function field(
  locale: FlowWebsiteLocale,
  definition: WorkflowNodeFieldDefinition & {
    labels: LocalizedText;
    help: LocalizedText;
  },
): WorkflowNodeFieldDefinition {
  const { labels, help, ...rest } = definition;
  return {
    ...rest,
    display_name: labels[locale],
    info: help[locale],
  };
}

const COPY = {
  name: text('审阅报关材料', 'Review customs documents'),
  description: text(
    '读取订单与报关附件，抽取申报字段并给出自动放行或人工复核建议。',
    'Read order context and customs attachments, extract declaration fields, and recommend automatic clearance or manual review.',
  ),
  orderContext: text('订单上下文', 'Order context'),
  orderContextHelp: text(
    '从画布连接已经完成库存分配的订单结果。',
    'Connect the order result produced after inventory allocation.',
  ),
  requiredDocuments: text('必需材料类型', 'Required document types'),
  requiredDocumentsHelp: text(
    '选择本次申报必须提供的材料，缺项会进入人工复核。',
    'Choose the document types required for this declaration; missing items require manual review.',
  ),
  files: text('报关附件', 'Customs attachments'),
  filesHelp: text(
    '上传发票、装箱单和原产地证明的示例文件。',
    'Attach example invoices, packing lists, and certificates of origin.',
  ),
  model: text('材料识别模型', 'Document extraction model'),
  modelHelp: text(
    '选择宿主已经授权的多模态材料识别模型。',
    'Choose a multimodal document model already authorized by the host.',
  ),
  prompt: text('抽取指令', 'Extraction instructions'),
  promptHelp: text(
    '说明需要抽取的申报字段，可使用 $ 选择工作流变量。',
    'Describe the declaration fields to extract and use $ to choose workflow variables.',
  ),
  code: text('预处理代码', 'Preprocessing code'),
  codeHelp: text(
    '在材料送入识别模型前统一文件名和币种格式。',
    'Normalize file names and currency values before document extraction.',
  ),
  decisions: text('允许的审阅结论', 'Allowed review decisions'),
  decisionsHelp: text(
    '声明运行时可以返回的结论，并保留明确的人工复核分支。',
    'Declare the decisions the runtime may return while preserving an explicit manual-review branch.',
  ),
  connector: text('海关数据连接器', 'Customs data connector'),
  connectorHelp: text(
    '配置宿主提供的海关目录查询服务和调用工具。',
    'Configure the host customs-catalog service and its lookup tool.',
  ),
  credential: text('凭据引用', 'Credential reference'),
  credentialHelp: text(
    '只保存密钥库引用，不在工作流中写入真实密钥。',
    'Store only a vault reference and never place a real secret in the workflow.',
  ),
  jurisdictions: text('适用辖区标签', 'Jurisdiction tags'),
  jurisdictionsHelp: text(
    '为运行时添加可搜索的申报辖区标签。',
    'Add searchable declaration-jurisdiction tags for the runtime.',
  ),
  preview: text('抽取结果预览', 'Extraction result preview'),
  previewHelp: text(
    '只读展示示例抽取结果，实际运行由宿主写入。',
    'Show an illustrative read-only extraction result; the host supplies the live value.',
  ),
} as const;

/** Host document-review node used by the complete Playground business workflow. */
export function createCustomsDocumentReviewNode(
  locale: FlowWebsiteLocale,
): A3SFlowCustomDagNodeRegistration {
  return defineA3SFlowCustomDagNode({
    manifest: {
      type: 'commerce.customs.document-review',
      display_name: COPY.name[locale],
      description: COPY.description[locale],
      category: 'custom',
      categoryLabel: locale === 'zh' ? '自定义节点' : 'Custom nodes',
      role: 'host',
      icon: 'file-search',
      ports: {
        inputs: [
          controlPort('in', locale === 'zh' ? '进入' : 'In'),
          dataPort('order', locale === 'zh' ? '订单' : 'Order', ['Json']),
        ],
        outputs: [
          controlPort(
            'next',
            locale === 'zh' ? '材料已就绪' : 'Documents ready',
          ),
          dataPort(
            'declaration',
            locale === 'zh' ? '申报字段' : 'Declaration',
            ['Json'],
          ),
          dataPort(
            'review_required',
            locale === 'zh' ? '需要复核' : 'Review required',
            ['Boolean'],
          ),
        ],
      },
      input_types: ['Json', 'File[]'],
      output_types: ['Json', 'Boolean'],
      fields: [
        field(locale, {
          name: 'order_context',
          labels: COPY.orderContext,
          help: COPY.orderContextHelp,
          type: 'other',
          _input_type: 'HandleInput',
          input_types: ['Json'],
          value: { source: 'input.order' },
          required: true,
          ui_group: 'documents',
          ui_group_label:
            locale === 'zh' ? '申报材料' : 'Declaration documents',
        }),
        field(locale, {
          name: 'required_document_types',
          labels: COPY.requiredDocuments,
          help: COPY.requiredDocumentsHelp,
          type: 'str',
          _input_type: 'MultiselectInput',
          value: ['commercial_invoice', 'packing_list'],
          list: true,
          options: [
            {
              label: locale === 'zh' ? '商业发票' : 'Commercial invoice',
              value: 'commercial_invoice',
            },
            {
              label: locale === 'zh' ? '装箱单' : 'Packing list',
              value: 'packing_list',
            },
            {
              label: locale === 'zh' ? '原产地证明' : 'Certificate of origin',
              value: 'certificate_of_origin',
            },
            {
              label: locale === 'zh' ? '运输单据' : 'Transport document',
              value: 'transport_document',
            },
          ],
          required: true,
          ui_group: 'documents',
          ui_group_label:
            locale === 'zh' ? '申报材料' : 'Declaration documents',
        }),
        field(locale, {
          name: 'document_files',
          labels: COPY.files,
          help: COPY.filesHelp,
          type: 'file',
          _input_type: 'FileInput',
          value: ['commercial-invoice.pdf', 'packing-list.pdf'],
          is_list: true,
          file_types: ['pdf', 'png', 'jpg'],
          required: true,
          ui_group: 'documents',
          ui_group_label:
            locale === 'zh' ? '申报材料' : 'Declaration documents',
        }),
        field(locale, {
          name: 'extraction_model',
          labels: COPY.model,
          help: COPY.modelHelp,
          type: 'model',
          _input_type: 'ModelInput',
          model_type: 'multimodal',
          value: 'document-vision-v3',
          options: [
            { label: 'Document Vision v3', value: 'document-vision-v3' },
            {
              label: 'Trade Document Extractor v2',
              value: 'trade-extractor-v2',
            },
          ],
          required: true,
          ui_group: 'extraction',
          ui_group_label: locale === 'zh' ? '材料识别' : 'Document extraction',
        }),
        field(locale, {
          name: 'extraction_prompt',
          labels: COPY.prompt,
          help: COPY.promptHelp,
          type: 'prompt',
          _input_type: 'PromptInput',
          value:
            'Extract HS codes, declared value, currency, and origin for order {{input.order_id}}.',
          required: true,
          ui_group: 'extraction',
          ui_group_label: locale === 'zh' ? '材料识别' : 'Document extraction',
        }),
        field(locale, {
          name: 'preprocess_code',
          labels: COPY.code,
          help: COPY.codeHelp,
          type: 'code',
          _input_type: 'CodeInput',
          language: 'typescript',
          file_path: 'customs/preprocess.ts',
          value:
            'export function preprocess(file: { name: string }) {\n  return { ...file, name: file.name.trim().toLowerCase() };\n}',
          required: true,
          ui_group: 'extraction',
          ui_group_label: locale === 'zh' ? '材料识别' : 'Document extraction',
        }),
        field(locale, {
          name: 'allowed_decisions',
          labels: COPY.decisions,
          help: COPY.decisionsHelp,
          type: 'actionPicker',
          _input_type: 'ActionPickerInput',
          value: ['clear', 'request_documents', 'manual_review'],
          required: true,
          ui_group: 'decision',
          ui_group_label: locale === 'zh' ? '审阅结论' : 'Review decisions',
        }),
        field(locale, {
          name: 'customs_connector',
          labels: COPY.connector,
          help: COPY.connectorHelp,
          type: 'mcp',
          _input_type: 'McpInput',
          value: {
            server: 'customs-catalog',
            tool: 'declaration.lookup',
            timeout_ms: 5_000,
          },
          advanced: true,
          ui_group: 'integration',
          ui_group_label: locale === 'zh' ? '宿主连接' : 'Host integration',
        }),
        field(locale, {
          name: 'credential_reference',
          labels: COPY.credential,
          help: COPY.credentialHelp,
          type: 'str',
          _input_type: 'SecretStrInput',
          value: 'vault://customs/sandbox',
          password: true,
          advanced: true,
          ui_group: 'integration',
          ui_group_label: locale === 'zh' ? '宿主连接' : 'Host integration',
        }),
        field(locale, {
          name: 'jurisdictions',
          labels: COPY.jurisdictions,
          help: COPY.jurisdictionsHelp,
          type: 'str',
          _input_type: 'StrInput',
          value: ['CN', 'EU'],
          list: true,
          advanced: true,
          ui_group: 'integration',
          ui_group_label: locale === 'zh' ? '宿主连接' : 'Host integration',
        }),
        field(locale, {
          name: 'result_preview',
          labels: COPY.preview,
          help: COPY.previewHelp,
          type: 'data_display',
          _input_type: 'DataDisplayInput',
          value: {
            status: 'ready',
            extracted_fields: 12,
            warnings: ['origin_requires_review'],
          },
          readonly: true,
          advanced: true,
          ui_group: 'result',
          ui_group_label: locale === 'zh' ? '结果预览' : 'Result preview',
        }),
      ],
      outputs: [
        {
          name: 'declaration',
          display_name: locale === 'zh' ? '申报字段' : 'Declaration',
          types: ['Json'],
          group_outputs: false,
          allows_loop: false,
          tool_mode: false,
        },
        {
          name: 'review_required',
          display_name: locale === 'zh' ? '需要复核' : 'Review required',
          types: ['Boolean'],
          group_outputs: false,
          allows_loop: false,
          tool_mode: false,
        },
      ],
    },
    capability: {
      id: 'commerce/customs-document-review',
      version: '1.0.0',
      handler: 'customs.review-documents',
    },
  });
}
