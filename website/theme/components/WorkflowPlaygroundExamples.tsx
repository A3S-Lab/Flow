import {
  ArrowLeft,
  ArrowRight,
  Brain,
  CheckCircle,
  Database,
  GitBranch,
  Pulse,
  ShieldCheck,
  Stack,
} from '@phosphor-icons/react';
import type { MouseEvent } from 'react';
import type {
  WorkflowExampleCategory,
  WorkflowExampleDefinition,
} from './WorkflowPlayground.examples';
import {
  workflowExamplesCopy,
  type WorkflowExamplesCopy,
} from './WorkflowPlayground.examples.copy';
import { SelectControl } from '@a3s-lab/flow-ui/react';
import type { PlaygroundNode } from './WorkflowPlayground.model';
import type { FlowWebsiteLocale } from './flow-node-catalog';

const categoryIcons: Readonly<Record<WorkflowExampleCategory, typeof Stack>> = {
  showcase: Stack,
  agent: Brain,
  approval: ShieldCheck,
  data: Database,
  recovery: Pulse,
};

function toneForNode(node: PlaygroundNode): string {
  const type = node.data.dagNode.data.type;
  if (type === 'flow.complete' || type === 'flow.progress') return 'green';
  if (
    type === 'flow.fail' ||
    type === 'flow.cancel' ||
    type === 'flow.timeout'
  ) {
    return 'red';
  }
  if (type === 'flow.wait' || type === 'flow.hook' || type === 'flow.signal') {
    return 'amber';
  }
  if (type === 'flow.condition' || type === 'iteration' || type === 'loop') {
    return 'cyan';
  }
  if (type.startsWith('flow.child')) return 'violet';
  return 'blue';
}

function nodeTitle(node: PlaygroundNode): string {
  const title = node.data.dagNode.data.title;
  return typeof title === 'string' && title.trim() ? title : node.id;
}

function WorkflowTopology({ example }: { example: WorkflowExampleDefinition }) {
  const nodes = example.graph.nodes.filter(({ parentId }) => !parentId);
  const nodeById = new Map(nodes.map((node) => [node.id, node] as const));
  const width = 208;
  const height = 54;
  const padding = 52;
  const minX = Math.min(...nodes.map(({ position }) => position.x));
  const minY = Math.min(...nodes.map(({ position }) => position.y));
  const maxX = Math.max(...nodes.map(({ position }) => position.x + width));
  const maxY = Math.max(...nodes.map(({ position }) => position.y + height));
  const viewWidth = Math.max(1, maxX - minX + padding * 2);
  const viewHeight = Math.max(1, maxY - minY + padding * 2);
  const offsetX = padding - minX;
  const offsetY = padding - minY;
  const showLabels = nodes.length <= 12;

  return (
    <svg
      aria-hidden="true"
      className="a3s-example-topology"
      preserveAspectRatio="xMidYMid meet"
      viewBox={`0 0 ${viewWidth} ${viewHeight}`}
    >
      <g className="a3s-example-topology__edges">
        {example.graph.edges.map((edge) => {
          const source = nodeById.get(edge.source);
          const target = nodeById.get(edge.target);
          if (!source || !target) return null;
          const sourceX = source.position.x + offsetX + width;
          const sourceY = source.position.y + offsetY + height / 2;
          const targetX = target.position.x + offsetX;
          const targetY = target.position.y + offsetY + height / 2;
          const bend = sourceX + (targetX - sourceX) / 2;
          return (
            <path
              d={`M ${sourceX} ${sourceY} C ${bend} ${sourceY}, ${bend} ${targetY}, ${targetX} ${targetY}`}
              key={edge.id}
            />
          );
        })}
      </g>
      <g className="a3s-example-topology__nodes">
        {nodes.map((node) => {
          const x = node.position.x + offsetX;
          const y = node.position.y + offsetY;
          return (
            <g className={`is-${toneForNode(node)}`} key={node.id}>
              <rect height={height} rx="9" width={width} x={x} y={y} />
              <circle cx={x + 20} cy={y + height / 2} r="7" />
              {showLabels && (
                <text x={x + 36} y={y + height / 2 + 4}>
                  {nodeTitle(node).slice(0, 24)}
                </text>
              )}
            </g>
          );
        })}
      </g>
    </svg>
  );
}

function WorkflowSequence({ example }: { example: WorkflowExampleDefinition }) {
  const nodes = example.graph.nodes
    .filter(({ parentId }) => !parentId)
    .slice()
    .sort((left, right) =>
      left.position.x === right.position.x
        ? left.position.y - right.position.y
        : left.position.x - right.position.x,
    )
    .slice(0, 6);

  return (
    <div aria-hidden="true" className="a3s-example-sequence">
      {nodes.map((node, index) => (
        <span className={`is-${toneForNode(node)}`} key={node.id}>
          <i />
          <em>{nodeTitle(node)}</em>
          {index < nodes.length - 1 && <b>→</b>}
        </span>
      ))}
    </div>
  );
}

function ExampleFacts({
  copy,
  example,
}: {
  copy: WorkflowExamplesCopy;
  example: WorkflowExampleDefinition;
}) {
  return (
    <div className="a3s-example-facts">
      <span>{copy.levels[example.level]}</span>
      <span>{copy.nodes(example.graph.nodes.length)}</span>
      <span>{copy.connections(example.graph.edges.length)}</span>
    </div>
  );
}

function handleExampleClick(
  event: MouseEvent<HTMLAnchorElement>,
  exampleId: string,
  onSelect: (exampleId: string) => void,
) {
  if (
    event.button !== 0 ||
    event.metaKey ||
    event.ctrlKey ||
    event.shiftKey ||
    event.altKey
  ) {
    return;
  }
  event.preventDefault();
  onSelect(exampleId);
}

type WorkflowPlaygroundExamplesProps = {
  examples: readonly WorkflowExampleDefinition[];
  homeHref: string;
  languageHref: string;
  locale: FlowWebsiteLocale;
  logoSrc: string;
  unknownExampleId?: string;
  version: string;
  versions: readonly string[];
  exampleHref: (exampleId: string) => string;
  versionHref: (version: string) => string;
  onSelect: (exampleId: string) => void;
  onVersionChange: (version: string) => void;
};

export function WorkflowPlaygroundExamples({
  examples,
  homeHref,
  languageHref,
  locale,
  logoSrc,
  unknownExampleId,
  version,
  versions,
  exampleHref,
  versionHref,
  onSelect,
  onVersionChange,
}: WorkflowPlaygroundExamplesProps) {
  const copy = workflowExamplesCopy[locale];
  const featured = examples.find(({ featured }) => featured) ?? examples[0];
  const remaining = examples.filter(({ id }) => id !== featured.id);

  return (
    <main
      className="a3s-workflow-playground a3s-workflow-examples"
      data-flow-playground=""
      data-language={locale}
      data-testid="workflow-example-library"
    >
      <header className="a3s-example-library-header">
        <div className="a3s-example-library-header__identity">
          <a aria-label={copy.backHome} href={homeHref} title={copy.backHome}>
            <ArrowLeft aria-hidden="true" />
          </a>
          <img alt="" src={logoSrc} />
          <div>
            <strong>Workflow Playground</strong>
            <small>{copy.localDraftNotice}</small>
          </div>
        </div>
        <div className="a3s-example-library-header__actions">
          <label className="a3s-example-library-header__version">
            <span className="a3s-visually-hidden">{copy.version}</span>
            <SelectControl
              aria-label={copy.version}
              className="a3s-example-library-header__version-select"
              id="workflow-example-library-version"
              onChange={(event) => onVersionChange(event.currentTarget.value)}
              value={version}
            >
              {versions.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </SelectControl>
          </label>
          <a
            aria-label={copy.language}
            href={languageHref}
            hrefLang={locale === 'zh' ? 'en' : 'zh-Hans'}
            title={copy.language}
          >
            {locale === 'zh' ? 'EN' : '中文'}
          </a>
        </div>
      </header>
      <noscript>
        {versions.map((item) => (
          <a href={versionHref(item)} key={item}>
            {item}
          </a>
        ))}
      </noscript>

      <div className="a3s-example-library">
        <header className="a3s-example-library__intro">
          <div>
            <h1>{copy.pageTitle}</h1>
            <p>{copy.pageDescription}</p>
          </div>
          <p>
            <CheckCircle aria-hidden="true" weight="fill" />
            {copy.pageDetail}
          </p>
        </header>

        {unknownExampleId && (
          <aside className="a3s-example-library__notice" role="alert">
            <GitBranch aria-hidden="true" />
            <div>
              <strong>{copy.unknownExample}</strong>
              <p>{copy.unknownExampleDetail}</p>
            </div>
            <code>{unknownExampleId}</code>
          </aside>
        )}

        <section
          aria-labelledby="featured-workflow-example"
          className="a3s-example-featured"
        >
          <div className="a3s-example-featured__content">
            <div className="a3s-example-featured__label">
              <Stack aria-hidden="true" weight="duotone" />
              <span>{copy.featured}</span>
            </div>
            <h2 id="featured-workflow-example">{featured.title}</h2>
            <p>{featured.description}</p>
            <ExampleFacts copy={copy} example={featured} />
            <div className="a3s-example-featured__detail">
              <strong>{copy.outcome}</strong>
              <p>{featured.outcome}</p>
            </div>
            <ul aria-label={copy.capabilities}>
              {featured.capabilities.map((capability) => (
                <li key={capability}>{capability}</li>
              ))}
            </ul>
            <a
              className="a3s-example-featured__action"
              href={exampleHref(featured.id)}
              onClick={(event) =>
                handleExampleClick(event, featured.id, onSelect)
              }
            >
              <span>{copy.openFeatured}</span>
              <ArrowRight aria-hidden="true" />
            </a>
          </div>
          <div className="a3s-example-featured__preview">
            <WorkflowTopology example={featured} />
            <p>{copy.featuredDetail}</p>
          </div>
        </section>

        <section
          aria-labelledby="workflow-example-list"
          className="a3s-example-list"
        >
          <header>
            <div>
              <h2 id="workflow-example-list">{copy.browseTitle}</h2>
              <p>{copy.browseDescription}</p>
            </div>
            <span>{remaining.length}</span>
          </header>
          <ul className="a3s-example-grid">
            {remaining.map((example) => {
              const Icon = categoryIcons[example.category];
              return (
                <li key={example.id}>
                  <a
                    aria-label={copy.openExample(example.title)}
                    className="a3s-example-card"
                    href={exampleHref(example.id)}
                    onClick={(event) =>
                      handleExampleClick(event, example.id, onSelect)
                    }
                  >
                    <span
                      aria-hidden="true"
                      className={`a3s-example-card__icon is-${example.category}`}
                    >
                      <Icon weight="duotone" />
                    </span>
                    <div className="a3s-example-card__copy">
                      <span>{copy.categories[example.category]}</span>
                      <h3>{example.title}</h3>
                      <p>{example.description}</p>
                    </div>
                    <span className="a3s-example-card__action">
                      <span>{copy.openExample(example.title)}</span>
                      <ArrowRight aria-hidden="true" />
                    </span>
                    <WorkflowSequence example={example} />
                    <ExampleFacts copy={copy} example={example} />
                  </a>
                </li>
              );
            })}
          </ul>
        </section>
      </div>
    </main>
  );
}
