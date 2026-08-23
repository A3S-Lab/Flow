import {
  ArrowRight,
  ArrowsClockwise,
  GitBranch,
  Robot,
  UserFocus,
  Wrench,
} from '@phosphor-icons/react';
import { useLang, useSite, useVersion, withBase } from '@rspress/core/runtime';
import { homeCopy, type HomeLocale } from './HomeCopy';
import {
  AgentRows,
  ArchitectureMap,
  DeveloperConsole,
  HeroWorkflowCanvas,
  NodeCatalogVisual,
  RecoveryTimeline,
} from './HomeVisuals';

function docHref(
  route: string,
  locale: HomeLocale,
  version: string,
  defaultVersion: string,
) {
  const prefix = [
    version !== defaultVersion ? version : '',
    locale === 'en' ? 'en' : '',
  ]
    .filter(Boolean)
    .join('/');
  return withBase(
    `/${[prefix, route.replace(/^\//, '')].filter(Boolean).join('/')}`,
  );
}

function MarkdownHome({ locale }: { locale: HomeLocale }) {
  const copy = homeCopy[locale];
  return (
    <main>
      <h1>{copy.hero.title.join(' ')}</h1>
      <p>{copy.hero.body}</p>
      <h2>{copy.engine.title}</h2>
      <p>{copy.engine.body}</p>
      <p>{copy.engine.detail}</p>
      <h2>{copy.authoring.title}</h2>
      <p>{copy.authoring.body}</p>
      <p>{copy.authoring.detail}</p>
      <h2>{copy.agents.title}</h2>
      <p>{copy.agents.body}</p>
      {copy.agents.rows.map((row) => (
        <section key={row.title}>
          <h3>{row.title}</h3>
          <p>{row.detail}</p>
        </section>
      ))}
      <h2>{copy.developer.title}</h2>
      <p>{copy.developer.body}</p>
      {copy.developer.items.map((item) => (
        <section key={item.id}>
          <h3>{item.title}</h3>
          <p>{item.detail}</p>
          <pre>
            <code>{item.code}</code>
          </pre>
        </section>
      ))}
      <h2>{copy.architecture.title}</h2>
      <p>{copy.architecture.body}</p>
    </main>
  );
}

const assuranceIcons = [ArrowsClockwise, Robot, UserFocus, GitBranch] as const;

export function HomeLayout() {
  const locale: HomeLocale = useLang() === 'en' ? 'en' : 'zh';
  const copy = homeCopy[locale];
  const { site } = useSite();
  const version = useVersion();
  const defaultVersion = site.multiVersion.default ?? version;
  const href = (route: string) =>
    docHref(route, locale, version, defaultVersion);

  if (import.meta.env.SSG_MD) return <MarkdownHome locale={locale} />;

  return (
    <main className="flow-home" data-flow-home>
      <section className="flow-hero">
        <div className="flow-hero__copy">
          <h1>
            <span>{copy.hero.title[0]}</span>
            <strong>{copy.hero.title[1]}</strong>
          </h1>
          <p>{copy.hero.body}</p>
          <div className="flow-hero__actions">
            <a className="flow-button is-primary" href={href('/guide/')}>
              {copy.hero.primary}
              <ArrowRight aria-hidden="true" size={16} weight="bold" />
            </a>
            <a className="flow-button is-secondary" href={href('/nodes/')}>
              {copy.hero.secondary}
            </a>
          </div>
        </div>
        <HeroWorkflowCanvas copy={copy.hero} locale={locale} />
      </section>

      <aside
        className="flow-assurance"
        aria-label={
          locale === 'zh' ? 'A3S Flow 核心能力' : 'A3S Flow capabilities'
        }
      >
        {copy.assurances.map((item, index) => {
          const Icon = assuranceIcons[index];
          return (
            <article key={item.title}>
              <Icon aria-hidden="true" size={19} weight="duotone" />
              <div>
                <strong>{item.title}</strong>
                <small>{item.detail}</small>
              </div>
            </article>
          );
        })}
      </aside>

      <section className="flow-chapter flow-engine-chapter">
        <div className="flow-chapter__copy">
          <h2>{copy.engine.title}</h2>
          <p>{copy.engine.body}</p>
          <p>{copy.engine.detail}</p>
          <a href={href('/concepts/execution-model')}>
            {copy.engine.link}
            <ArrowRight aria-hidden="true" size={15} weight="bold" />
          </a>
        </div>
        <RecoveryTimeline copy={copy.engine} />
      </section>

      <section className="flow-chapter flow-authoring-chapter">
        <header className="flow-chapter__heading">
          <h2>{copy.authoring.title}</h2>
          <div>
            <p>{copy.authoring.body}</p>
            <p>{copy.authoring.detail}</p>
            <a href={href('/nodes/')}>
              {copy.authoring.action}
              <ArrowRight aria-hidden="true" size={15} weight="bold" />
            </a>
          </div>
        </header>
        <NodeCatalogVisual copy={copy.authoring} locale={locale} />
      </section>

      <section className="flow-chapter flow-agent-chapter">
        <header className="flow-chapter__heading">
          <h2>{copy.agents.title}</h2>
          <p>{copy.agents.body}</p>
        </header>
        <AgentRows copy={copy.agents} />
      </section>

      <section className="flow-chapter flow-developer-chapter">
        <header className="flow-chapter__heading">
          <h2>{copy.developer.title}</h2>
          <p>{copy.developer.body}</p>
        </header>
        <DeveloperConsole copy={copy.developer} href={href} />
      </section>

      <section className="flow-chapter flow-architecture-chapter">
        <header className="flow-chapter__heading">
          <h2>{copy.architecture.title}</h2>
          <p>{copy.architecture.body}</p>
        </header>
        <ArchitectureMap copy={copy.architecture} />
      </section>

      <section className="flow-final">
        <div>
          <Wrench aria-hidden="true" size={22} weight="duotone" />
          <h2>{copy.final.title}</h2>
          <p>{copy.final.body}</p>
        </div>
        <div>
          <a className="flow-button is-primary" href={href('/guide/')}>
            {copy.final.primary}
            <ArrowRight aria-hidden="true" size={16} weight="bold" />
          </a>
          <a
            className="flow-button is-secondary"
            href={href('/concepts/execution-model')}
          >
            {copy.final.secondary}
          </a>
        </div>
      </section>
    </main>
  );
}
