import { useState, type KeyboardEvent } from 'react';
import {
  ArrowRight,
  Check,
  ClockCountdown,
  Copy,
  Database,
  GitBranch,
  PauseCircle,
  ShieldCheck,
  TreeStructure,
  Waveform,
} from '@phosphor-icons/react';
import { withBase } from '@rspress/core/runtime';

type ReplayStage = 'project' | 'decide' | 'commit' | 'resume';

const replayStages: Array<{
  body: string;
  code: string;
  id: ReplayStage;
  label: string;
  result: string;
}> = [
  {
    id: 'project',
    label: 'Project',
    body: 'Rebuild the current snapshot from immutable event history.',
    code: 'let snapshot = store.load(&run_id).await?;',
    result: 'Sequence 18 projected without mutable workflow state.',
  },
  {
    id: 'decide',
    label: 'Decide',
    body: 'Ask deterministic workflow code for one typed command.',
    code: 'runtime.run_workflow(invocation).await?',
    result: 'ScheduleStep("charge-card") selected for the same history.',
  },
  {
    id: 'commit',
    label: 'Commit',
    body: 'Validate the command and append resulting events at an expected sequence.',
    code: 'store.append(expected_sequence, events).await?;',
    result: 'A stale writer cannot replace the durable winner.',
  },
  {
    id: 'resume',
    label: 'Resume',
    body: 'Replay immediately, suspend on durable external state, or finish.',
    code: 'let current = engine.snapshot(&run_id).await?;',
    result: 'Any compatible worker can continue from committed history.',
  },
];

const guarantees = [
  {
    body: 'Every workflow decision becomes visible through typed append-only events.',
    icon: Database,
    title: 'History is authority',
  },
  {
    body: 'Input, retry, signal, hook, and build drift fail instead of changing an existing run.',
    icon: ShieldCheck,
    title: 'Replay fails closed',
  },
  {
    body: 'Timers, signals, hooks, and delayed retries release compute while the run waits.',
    icon: PauseCircle,
    title: 'Waiting is durable',
  },
  {
    body: 'Runtime build IDs and patch markers keep rollout decisions explicit and inspectable.',
    icon: GitBranch,
    title: 'Deployments stay compatible',
  },
];

const executionPath = [
  {
    detail:
      'A stable workflow ID, runtime build, entrypoint, and input enter the run.',
    title: 'Start intent',
  },
  {
    detail: 'Workflow code reads projected history and returns one command.',
    title: 'Deterministic decision',
  },
  {
    detail:
      'Flow commits events before any later replay can observe the outcome.',
    title: 'Expected-sequence append',
  },
  {
    detail: 'A worker replays, suspends, or reaches one terminal result.',
    title: 'Durable continuation',
  },
];

function copyWithSelection(value: string) {
  const textarea = document.createElement('textarea');
  textarea.value = value;
  textarea.setAttribute('readonly', '');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  document.body.append(textarea);
  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, value.length);

  try {
    return document.execCommand('copy');
  } finally {
    textarea.remove();
  }
}

function MarkdownHome() {
  return (
    <main>
      <h1>Work that waits. History that remembers.</h1>
      <p>
        A3S Flow runs durable Rust workflows from append-only history and
        resumes safely on any compatible worker.
      </p>
      <h2>Install</h2>
      <pre>
        <code>{'a3s-flow = "=1.0.0"'}</code>
      </pre>
      <h2>Core guarantees</h2>
      {guarantees.map((item) => (
        <section key={item.title}>
          <h3>{item.title}</h3>
          <p>{item.body}</p>
        </section>
      ))}
      <h2>One replay cycle</h2>
      {executionPath.map((item) => (
        <section key={item.title}>
          <h3>{item.title}</h3>
          <p>{item.detail}</p>
        </section>
      ))}
    </main>
  );
}

export function HomeLayout() {
  const [stage, setStage] = useState<ReplayStage>('project');
  const [copyState, setCopyState] = useState<
    'idle' | 'copying' | 'copied' | 'failed'
  >('idle');
  const activeStage = replayStages.find((item) => item.id === stage)!;
  const installCommand = 'a3s-flow = "=1.0.0"';

  if (import.meta.env.SSG_MD) return <MarkdownHome />;

  const moveStage = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
      return;
    }
    event.preventDefault();
    const nextIndex =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? replayStages.length - 1
          : (index +
              (event.key === 'ArrowRight' ? 1 : -1) +
              replayStages.length) %
            replayStages.length;
    const next = replayStages[nextIndex];
    setStage(next.id);
    document.getElementById(`flow-stage-${next.id}`)?.focus();
  };

  const copyInstall = async () => {
    setCopyState('copying');
    let selectionCopied = false;

    try {
      selectionCopied = copyWithSelection(installCommand);
    } catch {
      selectionCopied = false;
    }

    if (selectionCopied) {
      setCopyState('copied');
      return;
    }

    try {
      if (!navigator.clipboard) throw new Error('Clipboard API unavailable');
      await Promise.race([
        navigator.clipboard.writeText(installCommand),
        new Promise<never>((_, reject) =>
          window.setTimeout(
            () => reject(new Error('Clipboard timed out')),
            800,
          ),
        ),
      ]);
      setCopyState('copied');
    } catch {
      setCopyState('failed');
    }
  };

  return (
    <main className="flow-home" data-flow-home>
      <section className="flow-hero">
        <div className="flow-hero__copy">
          <span className="flow-eyebrow">A3S Flow</span>
          <h1>Work that waits. History that remembers.</h1>
          <p>
            Durable Rust workflows from append-only history, ready to resume on
            any compatible worker.
          </p>
          <div className="flow-actions">
            <a
              className="flow-button flow-button--primary"
              href={withBase('/guide/getting-started')}
            >
              Start building
              <ArrowRight aria-hidden="true" size={17} weight="bold" />
            </a>
            <a
              className="flow-button flow-button--secondary"
              href="https://github.com/A3S-Lab/Flow"
            >
              View on GitHub
            </a>
          </div>
        </div>

        <div
          aria-label="Interactive replay cycle"
          className="replay-demo"
          role="region"
        >
          <div className="replay-demo__header">
            <div>
              <Waveform aria-hidden="true" size={18} weight="duotone" />
              <strong>Replay cycle</strong>
            </div>
            <span>run / payment-1842</span>
          </div>
          <div
            aria-label="Replay cycle"
            className="replay-demo__tabs"
            role="tablist"
          >
            {replayStages.map((item, index) => (
              <button
                aria-controls="flow-replay-panel"
                aria-selected={stage === item.id}
                id={`flow-stage-${item.id}`}
                key={item.id}
                onClick={() => setStage(item.id)}
                onKeyDown={(event) => moveStage(event, index)}
                role="tab"
                tabIndex={stage === item.id ? 0 : -1}
                type="button"
              >
                <span>{index + 1}</span>
                {item.label}
              </button>
            ))}
          </div>
          <div
            aria-labelledby={`flow-stage-${stage}`}
            aria-live="polite"
            className="replay-demo__panel"
            id="flow-replay-panel"
            role="tabpanel"
          >
            <div>
              <span>Current decision</span>
              <h2>{activeStage.label}</h2>
              <p>{activeStage.body}</p>
            </div>
            <pre>
              <code>{activeStage.code}</code>
            </pre>
            <footer>
              <Check aria-hidden="true" size={15} weight="bold" />
              <span>{activeStage.result}</span>
            </footer>
          </div>
        </div>
      </section>

      <section aria-label="Flow facts" className="flow-facts">
        <div>
          <strong>Append-only</strong>
          <span>event history</span>
        </div>
        <div>
          <strong>Rust 1.88</strong>
          <span>minimum toolchain</span>
        </div>
        <div>
          <strong>4 stores</strong>
          <span>memory to PostgreSQL</span>
        </div>
        <div>
          <strong>64 children</strong>
          <span>per bounded batch</span>
        </div>
      </section>

      <section className="flow-install">
        <div>
          <h2>Add one durable boundary.</h2>
          <p>
            Start in memory, then keep the same engine contract as persistence
            and workers move into production.
          </p>
        </div>
        <div className="flow-install__command">
          <code>{installCommand}</code>
          <button
            aria-label={
              copyState === 'copied'
                ? 'Dependency copied'
                : copyState === 'copying'
                  ? 'Copying dependency'
                  : copyState === 'failed'
                    ? 'Copy failed'
                    : 'Copy dependency'
            }
            disabled={copyState === 'copying'}
            onClick={copyInstall}
            type="button"
          >
            {copyState === 'copied' ? (
              <Check aria-hidden="true" size={17} weight="bold" />
            ) : (
              <Copy aria-hidden="true" size={17} />
            )}
            <span aria-live="polite">
              {copyState === 'copied'
                ? 'Copied'
                : copyState === 'copying'
                  ? 'Copying'
                  : copyState === 'failed'
                    ? 'Retry'
                    : 'Copy'}
            </span>
          </button>
        </div>
      </section>

      <section className="flow-section flow-contract">
        <header>
          <h2>Durability is a contract, not a daemon.</h2>
          <p>
            Flow separates deterministic decisions from host-owned side effects,
            then makes every recovery boundary explicit.
          </p>
        </header>
        <div className="flow-contract__grid">
          {guarantees.map(({ body, icon: Icon, title }, index) => (
            <article
              className={index === 0 ? 'is-primary' : undefined}
              key={title}
            >
              <Icon aria-hidden="true" size={25} weight="duotone" />
              <div>
                <h3>{title}</h3>
                <p>{body}</p>
              </div>
            </article>
          ))}
        </div>
      </section>

      <section className="flow-section flow-cycle">
        <header>
          <h2>One replay cycle. Four explicit phases.</h2>
          <p>
            A committed event always exists before workflow code can observe its
            result on replay.
          </p>
        </header>
        <ol className="flow-cycle__steps">
          {executionPath.map((item, index) => (
            <li key={item.title}>
              <span>{String(index + 1).padStart(2, '0')}</span>
              <h3>{item.title}</h3>
              <p>{item.detail}</p>
            </li>
          ))}
        </ol>
        <div className="flow-cycle__asset">
          <img
            alt="Projection, deterministic command, committed events, and durable suspension in one replay cycle"
            decoding="async"
            loading="lazy"
            src={withBase('/assets/execution-model.svg')}
          />
        </div>
        <a
          className="flow-text-link"
          href={withBase('/concepts/execution-model')}
        >
          Read the execution model
          <ArrowRight aria-hidden="true" size={16} weight="bold" />
        </a>
      </section>

      <section className="flow-diagram">
        <div className="flow-diagram__copy">
          <TreeStructure aria-hidden="true" size={28} weight="duotone" />
          <h2>Portable graphs. Host-owned capabilities.</h2>
          <p>
            Flow validates DAG structure and derives a deterministic plan. The
            host binds each node type to authorized execution.
          </p>
          <a
            className="flow-text-link"
            href={withBase('/concepts/workflow-dag')}
          >
            Explore workflow definitions
            <ArrowRight aria-hidden="true" size={16} weight="bold" />
          </a>
        </div>
        <div className="flow-diagram__asset">
          <img
            alt="Workflow DAG authoring, compilation, capability binding, and durable execution"
            decoding="async"
            loading="lazy"
            src={withBase('/assets/workflow-dag.svg')}
          />
        </div>
      </section>

      <section className="flow-section flow-operations">
        <header>
          <h2>Start local. Keep the same replay rules.</h2>
          <p>
            Choose persistence and dispatch for the host you operate without
            changing workflow semantics.
          </p>
        </header>
        <div className="flow-operations__columns">
          <article>
            <Database aria-hidden="true" size={24} weight="duotone" />
            <h3>Persistence</h3>
            <p>
              In-memory and JSONL stores are built in. SQLite and PostgreSQL
              share the same event-store contract.
            </p>
            <a href={withBase('/operations/persistence')}>Compare stores</a>
          </article>
          <article>
            <ClockCountdown aria-hidden="true" size={24} weight="duotone" />
            <h3>Workers and rollout</h3>
            <p>
              Route exact runtime builds, preserve patch markers, and keep
              retries, signals, hooks, and timers observable.
            </p>
            <a href={withBase('/operations/production')}>Operate Flow</a>
          </article>
        </div>
      </section>

      <section className="flow-cta">
        <div>
          <h2>Build the first replay-safe run.</h2>
          <p>
            Implement one runtime, start with a stable run ID, and inspect the
            committed snapshot.
          </p>
        </div>
        <a
          className="flow-button flow-button--primary"
          href={withBase('/guide/getting-started')}
        >
          Open quick start
          <ArrowRight aria-hidden="true" size={17} weight="bold" />
        </a>
      </section>
    </main>
  );
}
