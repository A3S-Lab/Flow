# A3S Flow
<p align="center">
  <img src="assets/readme/hero.svg" width="100%" alt="A3S Flow commits workflow decisions to append-only history and resumes safely after worker replacement" />
</p>



<p align="center">
  <strong>Language / 语言:</strong>
  <a href="README.md">English</a> ·
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <strong>用于代理、工具、审批和子工作流程的 AI 原生工作流程引擎。</strong><br />
  使用 React 或 Vue 进行创作，使用 CLI 和 Skill 进行自动化，并从追加式历史记录中恢复每次运行。
</p>

<p align="center">
  <a href="https://github.com/A3S-Lab/Flow/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/A3S-Lab/Flow/actions/workflows/ci.yml/badge.svg?branch=main" /></a>
  <a href="https://github.com/A3S-Lab/Flow/actions/workflows/security.yml"><img alt="Security" src="https://github.com/A3S-Lab/Flow/actions/workflows/security.yml/badge.svg?branch=main" /></a>
  <a href="https://crates.io/crates/a3s-flow"><img alt="crates.io" src="https://img.shields.io/crates/v/a3s-flow.svg" /></a>
  <a href="https://docs.rs/a3s-flow"><img alt="docs.rs" src="https://docs.rs/a3s-flow/badge.svg" /></a>
  <a href="https://opensource.org/license/mit"><img alt="MIT license" src="https://img.shields.io/crates/l/a3s-flow.svg" /></a>
</p>

<p align="center">
  <a href="https://a3s-lab.github.io/Flow/">中文文档</a>·
  <a href="https://a3s-lab.github.io/Flow/en/">英文文档</a>·
  <a href="https://a3s-lab.github.io/Flow/playground/">工作流 Playground</a>·
  <a href="#quick-start">快速开始</a>·
  <a href="#execution-model">执行模型</a>·
  <a href="#capability-map">功能</a>·
  <a href="#workflow-dag">工作流DAG</a>·
  <a href="#production-operations">运维</a>·
  <a href="#examples-and-guides">示例</a>·
  <a href="#release-status">状态</a>
</p>

A3S Flow 是一个 AI 原生工作流引擎和 Rust SDK，用于必须生存的工作
进程重新启动、延迟重试、计时器、异步消息、回调、
以及 worker 替换。每一个有意义的转变都会追加到历史。
引擎根据历史记录投影状态并拒绝重放漂移而不是
默默地接受不同的决定。同一个存储库还维护
可重用的创作包、React 和 Vue 挂钩、CLI 和编码 Agent Skill
对 Flow 的版本化工作流程文档契约进行操作。

|当这种情况发生时 | Flow 保持其耐用性 |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------ |
|进程在步骤完成后终止 |重放提交的输出；已完成的工作不再被调用 |
|重试、计时器、信号或回调未准备好 |运行挂起而不保留内存中堆栈或工作线程 |
|父级启动一个或多个子级工作流程 |子工作流身份、政策和最终结果在跨流崩溃窗口中幸存下来 |
|新的工作流程代码推出 |运行时构建 ID 和不可变补丁标记在兼容的重放路径上保留历史记录 |
|多个工作人员同时追加 |预期序列写入选择一个持久的获胜者并拒绝过时的决策 |
|当对等方运行时，批处理同级失败 |在运行终端结果之前，不稳定的对等点会被永久标记为取消 |

> [!IMPORTANT]
> Flow 拥有工作流程图验证、追加式历史记录、持久重播以及
> 生命周期状态。主机拥有节点实现、授权、租户
> 外部的策略、凭证、工具访问和逻辑幂等性
> 效果。 A3S Cloud绑定了这些产品能力；它不重复
> Flow 的编译器或运行时。

## 编写组件、挂钩、CLI 和技能

`@a3s-lab/flow-ui` 是 Flow 工作流程的可重用创作包。它
保留节点目录、编辑器组件、框架挂钩、命令行工具、
以及同一清单和图表合同上的代理指令。

|表面|当前合同|
| ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|Playground|集成的可视化创作路线，具有 35 节点跨境订单履行示例，涵盖所有 20 个注册表清单、完整的清单合同检查、拖放、类型化连接、可编辑的仅演示边缘标签、A3S UI 配置表单、Worker 和 WebAssembly 布局、可见节点渲染、DAG 编译、DSL 检查以及主机可注入的 CLI/Skill/Copilot 扩展抽屉 |
|节点目录 |六个创作组中的 18 个公共清单，包含字段、默认值、端口、运行时绑定和持久节点标识 |
|反应 |节点预览和配置组件加上`useA3SFlowNode`用于受控就绪节点状态； `createA3SFlowDesignerContext` 和 `A3SFlowDesignerExtensionArea` 向主机扩展公开不可变的完整 DSL 和选择上下文 |
|视图 | `useA3SFlowNode` 可在同一节点对象、默认值和清单注册表上组合 |
|自定义节点 |具有 A3S UI 表单渲染、精确执行器功能和发布门的不可变主机目录 |
|命令行|用于节点发现和文件 CRUD 的 JSON-first `a3s-flow` 命令：`create`、`read`、`update`、`delete`、`validate`、`compile` 和 `digest`；更新包括保留稳定 ID 的节点移动和边缘重定向、作用域容器子放置、来自标准输入或文件的批量或 NDJSON 流式补丁、乐观摘要检查、空运行、原子写入和最终候选重新验证 |
|技能|可安装的`a3s-flow` 技能，可在创建、连接、验证、查看或安全编辑工作流程文件之前查询 CLI |

[Workflow Playground](https://a3s-lab.github.io/Flow/playground/)，
[React guide](https://a3s-lab.github.io/Flow/reference/react),
[Vue guide](https://a3s-lab.github.io/Flow/reference/vue),
[custom node guide](https://a3s-lab.github.io/Flow/reference/custom-nodes),
[CLI reference](https://a3s-lab.github.io/Flow/reference/cli)，以及
[Skill guide](https://a3s-lab.github.io/Flow/reference/agent-skill) 记录每个
表面。完整的节点目录和配置参考位于
[Workflow nodes](https://a3s-lab.github.io/Flow/nodes/)。

## 快速开始

添加 Flow 和异步运行时：

```toml
[dependencies]
a3s-flow = "=1.1.0"
async-trait = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
```

将确定性工作流程决策与副作用步骤分开：

```rust
use a3s_flow::{
    FlowEngine, FlowError, FlowRuntime, RuntimeCommand, StepInvocation,
    WorkflowInvocation, WorkflowSpec,
};
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

struct GreetingRuntime;

#[async_trait]
impl FlowRuntime for GreetingRuntime {
    async fn run_workflow(
        &self,
        invocation: WorkflowInvocation,
    ) -> a3s_flow::Result<RuntimeCommand> {
        let ctx = invocation.context();

        if let Some(output) = ctx.step_output("greet") {
            return Ok(ctx.complete(output.clone()));
        }

        Ok(ctx.schedule_step(
            "greet",
            "greet_user",
            json!({ "name": ctx.input()["name"] }),
        ))
    }

    async fn run_step(
        &self,
        invocation: StepInvocation,
    ) -> a3s_flow::Result<serde_json::Value> {
        match invocation.step_name.as_str() {
            "greet_user" => {
                let name = invocation.input["name"].as_str().unwrap_or("unknown");
                Ok(json!({ "message": format!("hello {name}") }))
            }
            step => Err(FlowError::Runtime(format!("unknown step: {step}"))),
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> a3s_flow::Result<()> {
    let engine = FlowEngine::in_memory(Arc::new(GreetingRuntime));
    let spec = WorkflowSpec::rust_embedded("demo.greeting", "0.1.0", "demo", "main");

    let run_id = engine
        .start_with_id("greeting-ada", spec, json!({ "name": "Ada" }))
        .await?;
    let snapshot = engine.snapshot(&run_id).await?;

    println!("status={:?} output={:?}", snapshot.status, snapshot.output);
    Ok(())
}
```

`start_with_id()` 使创建可以在运行 ID、工作流程规范、
和输入匹配。权威漂移会带来冲突。对于完整的打字
具有两个持久步骤的示例，运行：

```sh
cargo run --example sequential_steps
```

## 执行模型

<p align="center">
  <img src="assets/readme/execution-model.svg" width="100%" alt="A3S Flow projects history, asks workflow code for one command, commits resulting events, then replays or suspends" />
</p>

一个重播周期有四个明确的阶段：

1. 从不可变的历史中投影当前的`WorkflowRunSnapshot`。
2. 向 `FlowRuntime` 询问一个确定性 `RuntimeCommand`。
3. 验证命令并按预期顺序附加其事件。
4. 重放、在持久外部状态下暂停或达到一个最终结果。

该边界产生了具体的保证：

- 成功的步骤仅在以下时间后才对工作流程代码可见
  `StepCompleted`经久耐用。
- 一流的活动在之前和之前保留创建/启动/结果分类帐
  宿主副作用后。每次尝试都带有幂等密钥并且
  击剑令牌；主机可以通过检查点附加受保护的心跳
  `heartbeat_activity`。如果提供者响应丢失，则返回
  `FlowError::UnknownOutcome` 并将暂停的尝试与
  `resolve_unknown_activity` 在允许重试或完成之前。一个
  每次尝试 `timeout_ms` 截止日期遵循相同的规则：超时变为
  未知而不是自动重复重试。
- 重复使用具有不同输入、重试策略、截止日期、信号名称的 ID，
  回调令牌或元数据因非确定性重放而失败。
- 计时器、延迟重试、信号和挂钩在运行时释放计算
  暂停。
- `schedule_steps` 中的最终故障中止了未解决的兄弟未来，并且
  首先保留 `step_cancelled` 标记。标记记录外部
  副作用结果未知，因此主机可以协调稳定的尝试
  任何补偿重试之前的幂等性密钥。
- 崩溃恢复从键入的事件而不是内存中重建状态
  堆栈。

物理副作用边界有意为**至少一次**。如果一个
进程在外部效果成功后但在其输出提交之前终止，
再次尝试。步骤实施必须使用稳定的
从工作流和步骤标识派生的幂等性密钥。

## 能力图

以下合约在当前`main`分支上实施并得到支持
通过可运行的示例或集成测试。

|面积 |当前合同|证据|
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|耐用的脚步|顺序步骤、并发步骤批次、类型化输入/输出帮助程序、稳定 ID、进度和子操作引用 | [`sequential_steps`](examples/sequential_steps.rs)、[`batch_steps`](examples/batch_steps.rs) |
|持久活动 |一流的活动分类账，具有尝试 ID、幂等性密钥、隔离令牌、每次尝试的截止日期、重试、不可重试的失败、心跳检查点和显式的未知结果协调；现有步骤运行时保持兼容 | `first_class_activity_persists_identity_and_output`、`activity_heartbeat_persists_checkpoint_and_rejects_stale_fence`、`activity_timeout_persists_deadline_and_enters_unknown_state`、`unknown_activity_outcome_waits_for_fenced_reconciliation` |
|重试政策 |立即重试、固定延迟和具有确定性全抖动的上限指数退避；截止日期锚定在失败时间和耗尽可能会失败或返回到工作流程后备逻辑 | [`retry_backoff`](examples/retry_backoff.rs)、[`recoverable_step_failure`](examples/recoverable_step_failure.rs) |
|暂停 |持久计时器、声明的命名信号和令牌路由的挂钩/回调无需占用工作线程即可恢复 | [`scheduler_worker`](examples/scheduler_worker.rs)、[`workflow_signals`](examples/workflow_signals.rs)、[`hook_approval`](examples/hook_approval.rs) |
|取消 |清理感知取消进入`Cancelling`，重放稳定的清理步骤，并记录一次输入的终端结果；强制取消仍然明确| [`cancellation`](examples/cancellation.rs) |
|子工作流程 |一流的单个子级和有界并发批处理在执行前保留每个子级身份并恢复部分跨流进度 | [`child_workflow`](examples/child_workflow.rs)、[`child_workflow_batch`](examples/child_workflow_batch.rs) |
|历史悠久 | `continue_as_new` 关闭一个流并从具有确切继承的工作流权限的新链接流恢复 | [`continue_as_new`](examples/continue_as_new.rs) |
|安全推出 |精确的运行时构建路由在突变之前拒绝不兼容的工作人员；不可变的补丁标记保留新旧重放分支[`replay_safe_patch`](examples/replay_safe_patch.rs)、[rollout recipe](docs/COOKBOOK.md#replay-safe-workflow-patches) |
|坚持|内置内存和 JSONL 存储； SQLite 和 PostgreSQL 共享 `FlowEventStore` 合约和规范的 A3S ORM 迁移 | [`local_file_durability`](examples/local_file_durability.rs)、[`sqlite_durability`](examples/sqlite_durability.rs)、[`postgres_durability`](examples/postgres_durability.rs) |
|调度|推荐A3S Boot任务管理；嵌入式兼容性队列和 `FlowWorker` 仍然可用 | [`boot_task_policy`](examples/boot_task_policy.rs)、[`task_queue_durability`](examples/task_queue_durability.rs) |
|可观察性|具有有限恐慌/超时隔离、并发扇出、A3S 事件桥和修复感知本地 JSONL 审计接收器的提交后观察者镜像已提交的事件，而无需成为状态权威 | [`observer_fanout`](examples/observer_fanout.rs)、[`local_audit_log`](examples/local_audit_log.rs) |
|原生 TypeScript |可选的源编译、工件标识、依赖项清单验证和版本化 JSON 调用协议； Rust 仍然是持久的权威 | [`native_ts_preflight`](examples/native_ts_preflight.rs)、[protocol guide](docs/NATIVE_TYPESCRIPT.md) |

### 有限重试

重试策略是重播命令的一部分。指数政策充分衍生
来自不可变的运行、步骤和尝试身份的抖动，因此重新启动无法
更改选定的持久期限：

```rust
use a3s_flow::RetryPolicy;
use std::time::Duration;

let retry = RetryPolicy::exponential(
    8,
    Duration::from_secs(1),
    Duration::from_secs(30),
);

Ok(ctx.schedule_step_with_retry(
    "charge-card",
    "charge_card",
    input,
    retry,
))
```

### 有界子扇出

`start_child_workflows()` 验证整个批次，保留每个生成的
子进程运行 ID，然后同时推进兄弟进程。一个批次最多包含
`MAX_CHILD_WORKFLOW_BATCH_SIZE` 儿童（目前 64 岁），家长结果为
以持久请求顺序而不是完成顺序记录。

```rust
let children = items
    .into_iter()
    .enumerate()
    .map(|(index, item)| {
        ctx.child_workflow(
            format!("item-{index:04}"),
            child_spec.clone(),
            json!({ "item": item }),
        )
    })
    .collect();

Ok(ctx.start_child_workflows(children))
```

将较大的扇出分割成稳定的窗口，并仅在之后发出下一个窗口
目前的成果是持久的。单子和批量孩子共享相同的
`RequestCancellation` 和 `Abandon` 政策。

## 工作流 DAG

`WorkflowDsl` 是版本化的便携式文档合约。它的可执行文件
有效负载是`nodes`和`edges`的有向图。流程验证结构和
得出确定性计划；主机将每个节点的`data.type`绑定到
授权执行人。

<p align="center">
  <img src="assets/readme/workflow-dag.svg" width="100%" alt="A3S Flow takes nodes and edges through one structural compiler, then a host binds node capabilities before the durable runtime executes them" />
</p>

将相同的线形状编译成稳定的计划和语义标识：

```rust
use a3s_flow::WorkflowDag;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string("workflow.json")?;
    let graph = WorkflowDag::from_json(&source)?;
    let plan = graph.execution_plan()?;

    println!("order={:?}", plan.top_level());
    println!("digest={}", graph.execution_digest()?);
    Ok(())
}
```

编译器拒绝重复的 ID、丢失的端点、自边、循环、
无效的跨范围边缘以及格式错误的迭代或循环容器。未知
字段往返，而布局、选择和视口不影响
执行摘要。摘要格式 `v2` 与 UI 共享并遵循
JavaScript 数字格式和 UTF-16 键排序，拒绝不安全的整数，
并将嵌套限制为 256 层。边缘标签仅用于演示。安
空画布仍可作为草稿导入，但无法生成执行结果
计划。查看可运行的
[`workflow_dsl_import`](examples/workflow_dsl_import.rs) 示例。

## 生产运营

### 坚持

所有商店都保留相同的事件信封和重播合同。

|商店 |最适合|特色|
| -------------------- | --------------------------------------------------- | ---------- |
| `InMemoryEventStore` |测试和临时嵌入式工作|内置|
| `LocalFileEventStore` |单进程 JSONL 耐久性 |内置|
| `SqliteEventStore` |单节点持久应用| `sqlite` |
| `PostgresEventStore` |多进程工作者共享权威历史| `postgres` |

SQLite 和 PostgreSQL 使用 `a3s-orm` 进行类型化访问、校验和迁移、
投影验证的事务附加、主动挂钩路由、预定唤醒索引、
持久的投影检查点和整个历史保留。 `FlowEngine::checkpoint`
仅持续一次性物化状态； SQL 追加事务推进了这一点
缓存来自已验证的事件尾部，而读取则验证最新序列并
事件 ID 并在元数据过时或时回退到权威历史记录重播
腐败。 `FlowEngine::history_page` 暴露一个有界的独占序列游标
用于存档/导出和可见性重建的页面大小，无需加载整个
历史进入记忆。生产 PostgreSQL 部署运行迁移权限
分别核实后，方可接纳在职人员
规范的迁移分类账。参见[Upgrading to Flow 1.0](docs/UPGRADING_TO_V1.md)。

对于档案工作者，`FlowEngine::export_history_pages` 固定初始历史
提示，验证连续的序列页面，并调用主机拥有的接收器
一次一页。 Flow 保持事件日志的权威性；云或其他主机
选择存档格式、保留策略和目的地。

流任务队列暴露了租用防护、过时任务重新排队、死信
检查和行政`redrive_dead_lettered`操作。内置
本地和 PostgreSQL 队列使重复重新驱动变得安全；自定义队列适配器
必须明确执行重新驱动合约。

拥有兼容性`FlowWorker`循环的主机可以调用
`run_until_idle_bounded(limit)` 至多耗尽公平/背压预算
在投入其他工作之前。在任何租赁之前，零限额都会被拒绝
获得的；当无界排水时`run_until_idle()`仍然可用
故意的。

工人宣传版本化的`FlowWorkerCapabilities`合同。主办方应
租赁工作前请拨打`worker.ensure_compatible(&required)`；协议或
任务能力不匹配无法关闭。云仍然负责队列
准入、租户公平、安置和处理器生命周期。

事件桥为日志保留稳定的步骤/活动尝试关联，
跟踪和审计接收器，同时保持尝试 ID 和幂等性密钥不受干扰
`safe_metric_labels()`。

保留仅删除完整的合格延续/子组件，并且
留下校验和墓碑。 Flow 永远不会压缩事件流的一部分；
工作流程使用 `continue_as_new` 绑定重播历史记录，而无需重写
真相的来源。

### 调度和可选功能

|特色 |添加|
| -------------------- | ------------------------------------------------ |
| `native-ts`（默认）|本机 TypeScript 编译和调用适配器 |
| `sqlite` | SQLite 事件历史记录和保留 |
| `postgres` | PostgreSQL 历史和兼容性任务队列 |
| `boot` |推荐的 A3S Boot 任务管理器集成 |
| `a3s-event` |提交后 A3S 事件接收器 |

`BootFlowTaskManager` 拥有处理器注册、作业状态、重试/超时
策略、停滞作业处理、逻辑重复数据删除、启动和关闭。
`FlowWorker` 加上内存中、本地文件或 PostgreSQL 兼容性队列
仍然可供嵌入式主机使用。

### 原生 TypeScript

`NativeTsRuntime` 将 TypeScript 工作流程和步骤源代码编译为本机
工件并通过版本化 JSON 协议调用它。神器身份
绑定源、编译器后端、工作目录、协议、操作系统和
架构。在严格的编译器清单模式下，Flow 会验证完整的
原子发布之前和之后的依赖图。

安装编译器并在`PATH`上提供Bun（或设置`A3S_FLOW_BUN`）：

```sh
cargo install a3s-flow --version 1.1.0 --locked \
  --bin a3s-flow-native-compiler

a3s-flow-native-compiler capabilities
```

TypeScript 是一个适配器，而不是第二个 SDK、事件存储、调度程序或工作流程
生命周期。阅读[compiler and protocol contract](docs/NATIVE_TYPESCRIPT.md)。

## 所有权边界

|流量拥有|主机拥有 |
| -------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
|工作流文档/图形解析、结构不变量、确定性计划、语义摘要、有界创作字节 API/会话和规范操作编码 |节点语义、功能绑定、凭证和创作策略 |
|可重用的节点清单、配置组件、React 和 Vue 挂钩、CLI 命令以及工作流程创作技能 |产品特定的节点可用性、身份、授权、发布和托管编辑器行为 |
|仅追加运行历史记录和预期序列写入 |产品授权、租赁和发布生命周期 |
|重放验证和终止状态 |物理副作用的逻辑幂等性|
|步骤、重试、等待、信号、挂钩、子工作流、取消和延续生命周期 |一个步骤可能调用哪些工具和外部系统|
|运行时构建准入、补丁标记、调度、存储、工作人员和观察者合约 |部署策略、兼容构建声明和遥测目标 |

这种拆分使 Flow 作为唯一持久的编排权威保持可重用性
无需将 SDK 转变为托管产品控制平面。

## 示例和指南

从一个可执行路径开始，然后移至您需要的关注点。

|目标|示例或指南 |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
|第一个持久的工作流程 | [`sequential_steps`](examples/sequential_steps.rs) |
|并发持久步骤| [`batch_steps`](examples/batch_steps.rs) |
|固定/指数重试和回退 | [`retry_backoff`](examples/retry_backoff.rs)、[`recoverable_step_failure`](examples/recoverable_step_failure.rs) |
|补偿| [`compensation`](examples/compensation.rs) |
|计时器、信号和批准回调 | [`scheduler_worker`](examples/scheduler_worker.rs)、[`workflow_signals`](examples/workflow_signals.rs)、[`hook_approval`](examples/hook_approval.rs) |
|清理感知取消 | [`cancellation`](examples/cancellation.rs) |
|单个和批量子工作流程 | [`child_workflow`](examples/child_workflow.rs)、[`child_workflow_batch`](examples/child_workflow_batch.rs) |
|重放安全的代码更改 | [`replay_safe_patch`](examples/replay_safe_patch.rs) |
|本地和共享持久性| [`local_file_durability`](examples/local_file_durability.rs)、[`sqlite_durability`](examples/sqlite_durability.rs)、[`postgres_durability`](examples/postgres_durability.rs) |
|原生 TypeScript | [`native_ts_preflight`](examples/native_ts_preflight.rs)、[`native_ts_greeting`](examples/native_ts_greeting.rs) |
|工作流程定义导入 | [`workflow_dsl_import`](examples/workflow_dsl_import.rs) |

|参考|它拥有什么 |
| -------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| [Documentation website](https://a3s-lab.github.io/Flow/) |指导设置、执行概念、生产操作、运行时、示例和 API 映射 |
| [Architecture](docs/ARCHITECTURE.md) |事件源、重放、存储、调度程序和本机运行时边界 |
| [Cookbook](docs/COOKBOOK.md) |稳定的ID、重试、批量、计时器、挂钩、信号、取消和补偿|
| [Execution-kernel roadmap](docs/ROADMAP.md) |有序的内核工作、流程/云所有权、兼容性、规模和发布门 |
| [Functional plan](docs/FUNCTIONAL_PLAN.md) |能力级别证据、完成门、维护工作和非目标 |
| [API stability](docs/API_STABILITY.md) | SemVer、持久兼容性、MSRV 和 `1.0.0` 发布合约 |
| [Upgrading to Flow 1.0](docs/UPGRADING_TO_V1.md) |支持 v1 之前的历史/模式、部署、验证和回滚 |
| [API docs](https://docs.rs/a3s-flow) |公共 Rust 类型和方法 |
| [Security policy](SECURITY.md) |支持的发布、信任边界和私人报告 |

＃＃ 发展

从此箱子运行检查，而不是从 A3S monorepo 根运行检查：

```sh
cargo +1.88.0 check --all-targets --all-features --locked
cargo fmt --all -- --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
```

Rust 1.88 是支持的最低 Rust 版本。存储库配方提供了
更深的矩阵：

```sh
just deep-test-non-pg
A3S_FLOW_POSTGRES_URL=postgres://user:pass@localhost:5432/a3s_flow \
  just postgres-test
A3S_FLOW_NATIVE_TS_COMPILER=/path/to/a3s-flow-native-compiler \
  just native-ts-bun-test
```

CI 还检查公共 API 兼容性、功能矩阵、真正的 PostgreSQL
门、包内容以及 Linux 和 Windows 上的端到端 Bun 工作流程。

## 发布状态

该crate当前声明版本 `1.1.0`。此兼容的次要版本
添加有界并发子工作流批次、有界指数重试、
并强化自定义工作流程节点创作，同时保留 Flow 1.x
运行时、重放和持久性合约。

可重用的工作流程创作组件、React 和 Vue 挂钩、CLI 以及
技能保存在此存储库中。托管租赁、授权、
特定于产品的功能绑定、部署策略和多租户
控制平面保留在 Rust 箱之外； A3S Cloud 拥有这些产品
表面。

维护以合同为主导：保留 SemVer 和重播兼容性，保持
SQLite/PostgreSQL 奇偶校验门对齐，跟踪本机编译器协议并
支持的目标，并仅为具体部署添加适配器。的
[functional plan](docs/FUNCTIONAL_PLAN.md)是能力的真实来源
证据和释放门。

＃＃ 执照

[MIT](https://opensource.org/license/mit) &复制； A3S实验室
