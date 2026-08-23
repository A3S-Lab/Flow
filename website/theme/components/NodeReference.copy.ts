export type NodeGuide = {
  use: { zh: string; en: string };
  behavior: { zh: string; en: string };
  notes: { zh: readonly string[]; en: readonly string[] };
};

export const nodeGuides: Readonly<Record<string, NodeGuide>> = {
  'flow.start': {
    use: {
      zh: '每张可执行图都从这个节点进入。它固定工作流名称、版本、输入结构和运行入口，并把启动输入交给后续节点。',
      en: 'Every executable graph enters through this node. It pins workflow identity, input shape, and runtime entry, then exposes the start input to downstream nodes.',
    },
    behavior: {
      zh: '开始节点参与发布校验，不会生成运行命令。运行 ID 表达式有值时，宿主可以用同一业务身份安全重试启动请求。',
      en: 'The start node is validated at publication time and emits no runtime command. A run ID expression lets the host retry a start request with the same business identity.',
    },
    notes: {
      zh: [
        '一张顶层图只保留一个开始节点。',
        '已有运行后不要修改工作流名称或版本。',
        '输入结构应只声明工作流真正读取的字段。',
      ],
      en: [
        'Keep one start node in a top-level graph.',
        'Do not change workflow identity for existing runs.',
        'Declare only input fields that workflow code actually reads.',
      ],
    },
  },
  'flow.step': {
    use: {
      zh: '用它调用一个已经在宿主注册的任务，例如工具、HTTP 请求、数据库写入或业务服务。每次任务有独立输入、结果和重试策略。',
      en: 'Use it for one host-registered task such as a tool call, HTTP request, database write, or business service. The task owns one input, result, and retry policy.',
    },
    behavior: {
      zh: '节点 ID 绑定持久步骤身份。结果提交历史后才会进入成功端口，重试耗尽时根据策略结束运行或开放失败端口。',
      en: 'The graph node ID becomes the durable step identity. The success port opens after a result is committed; exhausted retries either fail the run or expose the failure port.',
    },
    notes: {
      zh: [
        '宿主任务必须能处理至少一次交付。',
        '外部幂等键应由运行 ID、节点 ID 和尝试次数构造。',
        '只有选择继续失败分支时才连接 failed 端口。',
      ],
      en: [
        'The host task must tolerate at-least-once delivery.',
        'Derive external idempotency from run, node, and attempt identity.',
        'Connect the failed port only when exhaustion continues through the graph.',
      ],
    },
  },
  'flow.batch': {
    use: {
      zh: '用它声明一组可以独立推进的宿主任务。成员按固定顺序保存，各自拥有名称、输入和重试策略。',
      en: 'Use it to declare host tasks that can advance independently. Members are stored in a fixed order and keep separate names, inputs, and retry policies.',
    },
    behavior: {
      zh: '节点 ID 与成员 key 共同构成步骤身份。引擎先保存整批成员，再推进尚未完成的任务，结果数组按声明顺序汇总。',
      en: 'The node ID and member key form each durable step identity. The engine records the whole batch before advancing unfinished work and reports results in declaration order.',
    },
    notes: {
      zh: [
        '成员 key 必须非空且不能重复。',
        '不要按完成先后为成员重新编号。',
        '大批量任务应按输入中的稳定游标分窗。',
      ],
      en: [
        'Member keys must be non-empty and unique.',
        'Never renumber members by completion order.',
        'Window large batches with a stable cursor from workflow input.',
      ],
    },
  },
  'flow.condition': {
    use: {
      zh: '用它根据工作流数据选择两条控制分支。条件可以读取输入或上游数据端口，分支名称只影响画布显示。',
      en: 'Use it to choose between two control branches from workflow data. The expression may read start input or an upstream data port; branch labels affect presentation only.',
    },
    behavior: {
      zh: '条件在确定性的图执行阶段求值，不会调用宿主任务。表达式结果为真时进入 matched，其余情况进入 otherwise。',
      en: 'The condition evaluates during deterministic graph execution and does not call a host task. A true result selects matched; every other result selects otherwise.',
    },
    notes: {
      zh: [
        '表达式只读取已经持久化的数据。',
        '不要在条件中读取时钟、随机数或远程状态。',
        '两条控制分支都应连接到明确的后续节点。',
      ],
      en: [
        'Read only data already committed to history.',
        'Do not read clocks, randomness, or remote state in an expression.',
        'Connect both control branches to explicit downstream behavior.',
      ],
    },
  },
  'flow.wait': {
    use: {
      zh: '用它把运行暂停到一个绝对 UTC 时间。适合冷却期、预约执行、业务截止时间和确定性的重试窗口。',
      en: 'Use it to suspend a run until an absolute UTC instant. It fits cooling periods, scheduled work, business deadlines, and deterministic retry windows.',
    },
    behavior: {
      zh: '等待时间写入历史后，worker 会释放。调度器到期后重新投递运行，重放从 resumed 端口继续。',
      en: 'After the deadline is committed, the worker is released. The scheduler redelivers the run when due and replay continues from the resumed port.',
    },
    notes: {
      zh: [
        '使用带时区的绝对时间。',
        '进程恢复不会重新计算截止时间。',
        '生产环境需要调度扫描或定向唤醒。',
      ],
      en: [
        'Use an absolute timestamp with a timezone.',
        'Process recovery must not recalculate the deadline.',
        'Production needs scheduled scanning or targeted wake-up delivery.',
      ],
    },
  },
  'flow.hook': {
    use: {
      zh: '用它等待一次人工审批或外部回调。人工模式保留主题和元数据，webhook 模式还会显示请求方法与回调路径。',
      en: 'Use it for one human approval or external callback. Human mode keeps a subject and metadata; webhook mode also exposes the method and callback path.',
    },
    behavior: {
      zh: '节点创建带稳定身份的 Hook 并暂停运行。一次有效接收会开放 received 和 payload，关闭或取消则进入 disposed。',
      en: 'The node creates a durably identified hook and suspends the run. One valid receipt exposes received and payload; disposal or cancellation selects disposed.',
    },
    notes: {
      zh: [
        '回调 token 属于凭据，日志中不要输出。',
        '同一个 Hook 只能接收或关闭一次。',
        '取消请求会关闭当前仍在等待的 Hook。',
      ],
      en: [
        'Treat callback tokens as credentials and keep them out of logs.',
        'One hook can be received or disposed only once.',
        'A cancellation request disposes hooks that are still waiting.',
      ],
    },
  },
  'flow.complete': {
    use: {
      zh: '用它提交成功终态和返回值。输出表达式可以组合输入与前序节点结果，值会完整写入运行历史。',
      en: 'Use it to commit a successful terminal outcome and return value. The output expression can combine input and prior node results, and the full value is stored in history.',
    },
    behavior: {
      zh: '完成事件提交后，该运行段不再接受新事件。仍有需要父运行等待的子工作流时，引擎会拒绝提前结束。',
      en: 'After completion commits, the run segment accepts no later events. The engine rejects early completion while a child workflow still requires parent ownership.',
    },
    notes: {
      zh: [
        '成功输出应保持可序列化。',
        '取消清理路径不能以成功结束。',
        '大型结果宜保存对象引用而非完整内容。',
      ],
      en: [
        'Keep successful output serializable.',
        'A cancellation cleanup path cannot complete successfully.',
        'Store object references instead of very large result bodies.',
      ],
    },
  },
  'flow.fail': {
    use: {
      zh: '用它在工作流已经确认无法继续时提交失败终态。错误表达式应给操作者足够的业务上下文。',
      en: 'Use it when workflow logic has determined that the run cannot continue. The error expression should give operators actionable business context.',
    },
    behavior: {
      zh: '失败事件会关闭当前运行段，并保留表达式得到的错误文本。它与步骤重试耗尽产生的失败结果可以分别统计。',
      en: 'The failure event closes the current segment and stores the evaluated message. It remains distinguishable from a step failure caused directly by retry exhaustion.',
    },
    notes: {
      zh: [
        '错误文本不要包含 token 或完整外部响应。',
        '清理失败可以在取消过程中使用此终态。',
        '可恢复错误应先走步骤失败分支。',
      ],
      en: [
        'Keep tokens and complete external responses out of error text.',
        'Cleanup failure may use this outcome during cancellation.',
        'Recoverable errors should use the step failure branch first.',
      ],
    },
  },
  'flow.cancel': {
    use: {
      zh: '用它结束已经收到持久取消请求的运行。工作流通常先释放锁、撤回任务或等待子工作流清理，再进入此节点。',
      en: 'Use it to finish a run that already has a durable cancellation request. Workflow code normally releases resources or waits for child cleanup before reaching this node.',
    },
    behavior: {
      zh: '节点没有配置字段。缺少取消请求时，引擎拒绝这个终态；成功提交后，取消原因沿用原请求。',
      en: 'The node has no settings. The engine rejects it without a cancellation request, and the committed outcome keeps the reason from that request.',
    },
    notes: {
      zh: [
        '不要把取消节点当作普通分支出口。',
        '清理任务使用新的稳定节点身份。',
        '仍在清理的受管子工作流会阻止父运行结束。',
      ],
      en: [
        'Do not use cancellation as an ordinary branch exit.',
        'Give cleanup work fresh stable node identities.',
        'Managed children still cleaning up block the parent outcome.',
      ],
    },
  },
  'flow.timeout': {
    use: {
      zh: '用它记录一个已经超过的业务截止时间。deadline 保存原始 UTC 时间，reason 用来说明哪个等待窗口到期。',
      en: 'Use it to record a business deadline that has expired. The deadline keeps the original UTC instant, while reason identifies the elapsed window.',
    },
    behavior: {
      zh: '超时会关闭运行段，并保留独立的超时终态。通常先用等待节点持久化同一个截止时间，到期后再进入本节点。',
      en: 'Timeout closes the segment with a distinct timed-out outcome. A common graph first persists the same deadline in a wait node, then reaches timeout after wake-up.',
    },
    notes: {
      zh: [
        '不要在重放中临时读取当前时间决定超时。',
        'deadline 必须是确定的绝对时间。',
        'reason 只写简短业务说明。',
      ],
      en: [
        'Do not decide timeout from a fresh clock read during replay.',
        'The deadline must be a deterministic absolute instant.',
        'Keep reason to a short business explanation.',
      ],
    },
  },
  'flow.continue-as-new': {
    use: {
      zh: '用它结束当前历史段，并以新输入创建后续运行。适合长期轮询、分页处理和需要控制单段历史长度的流程。',
      en: 'Use it to close the current history segment and create a successor with new input. It fits long polling, paged processing, and flows that must bound segment history.',
    },
    behavior: {
      zh: '后续运行继承工作流定义、运行时构建和补丁标记。successor 端口用于编辑器表达关系，不会让旧运行继续执行。',
      en: 'The successor inherits workflow definition, runtime build, and patch markers. The successor port expresses graph intent; the closed run does not continue executing.',
    },
    notes: {
      zh: [
        '新输入应带上确定性的游标和累计状态。',
        '未消费信号存在时不能续段。',
        '取消清理过程中不能创建后续运行。',
      ],
      en: [
        'Carry a deterministic cursor and accumulated state in the new input.',
        'A run cannot continue while signals remain unconsumed.',
        'Cancellation cleanup cannot create a successor run.',
      ],
    },
  },
  'flow.progress': {
    use: {
      zh: '用它写入可检查的运行里程碑、计数和短说明。进度不会调用宿主任务，也不会替代业务状态。',
      en: 'Use it to write inspectable milestones, counts, and short operator messages. Progress does not call a host task and is not a substitute for business state.',
    },
    behavior: {
      zh: '节点 ID 绑定进度命令身份，progress_id 标识具体更新。提交后运行立即重放，并从 recorded 端口继续。',
      en: 'The node ID identifies the command and progress_id identifies the update. After commit, the run replays immediately and continues through recorded.',
    },
    notes: {
      zh: [
        '同一个 progress_id 不要写入不同内容。',
        'total 有值时应不小于 completed。',
        'details 只保存小型结构数据或对象引用。',
      ],
      en: [
        'Do not reuse one progress_id for changed content.',
        'When present, total must not be lower than completed.',
        'Keep details to small structured data or object references.',
      ],
    },
  },
  'flow.child-operation': {
    use: {
      zh: '用它登记由其他系统拥有的长任务，例如渲染、导入或外部批处理。节点只保存引用，不会启动或等待该任务。',
      en: 'Use it to register a long-running job owned by another system, such as rendering, import, or an external batch. The node stores a reference and does not start or await the job.',
    },
    behavior: {
      zh: '引用提交后从 linked 端口继续。后续轮询、取消和结果收集需要单独的步骤、Hook 或信号节点。',
      en: 'After the reference commits, execution continues through linked. Polling, cancellation, and result collection require separate step, hook, or signal nodes.',
    },
    notes: {
      zh: [
        'reference_id 在父运行内保持稳定。',
        'operation_id 使用外部所有者分配的真实身份。',
        '需要完整父子生命周期时改用子工作流。',
      ],
      en: [
        'Keep reference_id stable within the parent run.',
        'Use the real identity assigned by the external owner.',
        'Choose a child workflow when Flow must own the full lifecycle.',
      ],
    },
  },
  'flow.child-workflow': {
    use: {
      zh: '用它启动一个拥有独立输入、历史、信号和终态的子工作流。父运行会等待子运行解析后再继续。',
      en: 'Use it to start one child workflow with independent input, history, signals, and terminal outcome. The parent waits until the child resolves.',
    },
    behavior: {
      zh: '节点 ID 和 child_id 固定父子关系。引擎先保存请求，再启动或恢复子运行，完成后开放 completed 与 outcome。',
      en: 'The node ID and child_id pin the parent-child relationship. The engine stores the request before starting or recovering the child, then exposes completed and outcome.',
    },
    notes: {
      zh: [
        '同一个 child_id 的 spec 与输入必须保持不变。',
        'request_cancellation 会让父运行等待子项清理。',
        'abandon 允许子运行脱离父项继续。',
      ],
      en: [
        'Keep spec and input unchanged behind one child_id.',
        'request_cancellation makes the parent wait for child cleanup.',
        'abandon lets the child continue after parent ownership ends.',
      ],
    },
  },
  'flow.child-workflows': {
    use: {
      zh: '用它一次声明多个彼此独立的子工作流。适合父运行需要统一等待并按稳定成员顺序收集结果的场景。',
      en: 'Use it to declare multiple independent child workflows at once. It fits a parent that must wait for all members and collect outcomes in stable order.',
    },
    behavior: {
      zh: '节点 ID 与每个 child_id 共同绑定成员身份。引擎先校验并保存整批请求，再协调子运行，最多接受 64 个成员。',
      en: 'The node ID and each child_id form member identity. The engine validates and records the complete request set before coordinating children, with a maximum of 64 members.',
    },
    notes: {
      zh: [
        '成员列表不能为空，child_id 不能重复。',
        '按声明顺序汇总结果，不依赖完成先后。',
        '更大的集合按稳定游标拆分成多批。',
      ],
      en: [
        'The list cannot be empty and child_id values must be unique.',
        'Aggregate by declaration order, not completion order.',
        'Split larger collections into batches with a stable cursor.',
      ],
    },
  },
  'flow.signal': {
    use: {
      zh: '用它等待一个具名外部消息。适合同名消息会多次到达、需要排队，并由业务发送方提供幂等身份的场景。',
      en: 'Use it to wait for a named external message. It fits repeated messages that must queue and carry caller-owned idempotency identity.',
    },
    behavior: {
      zh: '等待声明提交后释放 worker。最早到达且尚未消费的同名信号会绑定 wait_id，随后开放 received 与 payload。',
      en: 'The worker is released after the wait commits. The earliest unconsumed signal with the same name binds to wait_id, then exposes received and payload.',
    },
    notes: {
      zh: [
        'wait_id 在一次等待中保持稳定。',
        'signal_name 应来自发布时声明的信号契约。',
        '一次性公开 token 回调更适合 Hook。',
      ],
      en: [
        'Keep wait_id stable for one wait.',
        'Choose signal_name from the published signal contract.',
        'Use a hook for one callback protected by a public token.',
      ],
    },
  },
  iteration: {
    use: {
      zh: '用它为集合中的每个成员建立一个子画布作用域。items 表达式提供集合，容器内部从唯一的 iteration-start 开始。',
      en: 'Use it to create a child-canvas scope for each collection member. The items expression supplies the collection and the nested graph starts at one iteration-start node.',
    },
    behavior: {
      zh: '容器只描述图结构，不直接生成运行命令。宿主把子画布编译成稳定步骤或子工作流，并负责成员变量、并发和结果聚合。',
      en: 'The container describes graph structure and emits no runtime command by itself. The host compiles the child canvas into stable steps or child workflows and owns member variables, concurrency, and aggregation.',
    },
    notes: {
      zh: [
        'start_node_id 必须指向容器内的 iteration-start。',
        '容器至少还要有一个可执行子节点。',
        '连线不能跨越容器作用域。',
      ],
      en: [
        'start_node_id must point to the nested iteration-start.',
        'Add at least one executable child besides the start marker.',
        'Edges cannot cross the container boundary.',
      ],
    },
  },
  loop: {
    use: {
      zh: '用它建立带条件和次数上限的循环子画布。每次进入子作用域前都会检查 condition，并由 max_iterations 提供硬上限。',
      en: 'Use it for a loop child canvas with a condition and a hard iteration cap. The condition is checked before entering the nested scope each time.',
    },
    behavior: {
      zh: '容器本身不生成运行命令。宿主负责把每轮子图编译成持久决定，并在条件为假或达到次数上限时从 done 端口继续。',
      en: 'The container emits no runtime command on its own. The host compiles each pass into durable decisions and continues through done when the condition is false or the cap is reached.',
    },
    notes: {
      zh: [
        'start_node_id 必须指向容器内的 loop-start。',
        '使用 max_iterations 防止无法退出的流程。',
        '循环由容器表达，不要画回边。',
      ],
      en: [
        'start_node_id must point to the nested loop-start.',
        'Use max_iterations to bound a loop that cannot exit.',
        'Express repetition with the container, never a back edge.',
      ],
    },
  },
};
