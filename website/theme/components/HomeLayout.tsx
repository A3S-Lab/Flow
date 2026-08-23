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
import { HomeInteraction } from './HomeInteraction';
import {
  AgentRows,
  ArchitectureMap,
  DeveloperConsole,
  FlowSystemMap,
  HeroWorkflowCanvas,
  NodeCatalogVisual,
  RecoveryTimeline,
} from './HomeVisuals';

function ChapterMeta({ chapter }: { chapter: readonly [string, string] }) {
  return (
    <div className="flow-chapter__meta">
      <span>{chapter[0]}</span>
      <i aria-hidden="true" />
      <strong>{chapter[1]}</strong>
    </div>
  );
}

function HeroProductTitle({
  locale,
  title,
}: {
  locale: HomeLocale;
  title: string;
}) {
  if (locale !== 'zh') return title;

  const breakAt = title.indexOf('工作流');
  if (breakAt < 1) return title;

  return (
    <>
      <span className="flow-hero__title-line">{title.slice(0, breakAt)}</span>
      <span className="flow-hero__title-line">{title.slice(breakAt)}</span>
    </>
  );
}

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
      <h2>{copy.system.title}</h2>
      <p>{copy.system.body}</p>
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
  const localeHref = (targetLocale: HomeLocale) =>
    docHref('/', targetLocale, version, defaultVersion);

  if (import.meta.env.SSG_MD) return <MarkdownHome locale={locale} />;

  return (
    <main className="flow-home" data-flow-home>
      <HomeInteraction />
      <section className="flow-hero">
        <div className="flow-hero__copy">
          <div className="flow-hero__meta">
            <span>
              A3S FLOW <i aria-hidden="true">·</i> {copy.hero.meta}
            </span>
            <nav aria-label={copy.hero.languageLabel}>
              <a
                aria-current={locale === 'zh' ? 'page' : undefined}
                href={localeHref('zh')}
              >
                中文
              </a>
              <a
                aria-current={locale === 'en' ? 'page' : undefined}
                href={localeHref('en')}
              >
                EN
              </a>
            </nav>
          </div>
          <h1>
            <span>{copy.hero.title[0]}</span>
            <strong>
              <HeroProductTitle locale={locale} title={copy.hero.title[1]} />
            </strong>
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
        <strong>{copy.assuranceTitle}</strong>
        <div className="flow-assurance__items">
          {copy.assurances.map((item, index) => {
            const Icon = assuranceIcons[index];
            return (
              <article key={item.title}>
                <span>
                  <Icon aria-hidden="true" size={19} weight="duotone" />
                </span>
                <div>
                  <strong>{item.title}</strong>
                  <small>{item.detail}</small>
                </div>
              </article>
            );
          })}
        </div>
      </aside>

      <section className="flow-system-intro" id="product-system">
        <div data-reveal>
          <span className="flow-section-eyebrow">{copy.system.eyebrow}</span>
          <h2>{copy.system.title}</h2>
          <p>{copy.system.body}</p>
        </div>
        <div data-reveal>
          <FlowSystemMap copy={copy.system} />
        </div>
      </section>

      <section className="flow-chapter flow-engine-chapter" id="durable-engine">
        <div className="flow-chapter__copy" data-reveal>
          <ChapterMeta chapter={copy.engine.chapter} />
          <h2>{copy.engine.title}</h2>
          <p>{copy.engine.body}</p>
          <p>{copy.engine.detail}</p>
          <a href={href('/concepts/execution-model')}>
            {copy.engine.link}
            <ArrowRight aria-hidden="true" size={15} weight="bold" />
          </a>
        </div>
        <div data-reveal>
          <RecoveryTimeline copy={copy.engine} />
        </div>
      </section>

      <section
        className="flow-chapter flow-authoring-chapter"
        id="frontend-components"
      >
        <header className="flow-chapter__heading" data-reveal>
          <ChapterMeta chapter={copy.authoring.chapter} />
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
        <div data-reveal>
          <NodeCatalogVisual copy={copy.authoring} locale={locale} />
        </div>
      </section>

      <section className="flow-chapter flow-agent-chapter" id="ai-workloads">
        <header className="flow-chapter__heading" data-reveal>
          <ChapterMeta chapter={copy.agents.chapter} />
          <h2>{copy.agents.title}</h2>
          <p>{copy.agents.body}</p>
        </header>
        <div data-reveal>
          <AgentRows copy={copy.agents} />
        </div>
      </section>

      <section
        className="flow-chapter flow-developer-chapter"
        id="developer-surfaces"
      >
        <header className="flow-chapter__heading" data-reveal>
          <ChapterMeta chapter={copy.developer.chapter} />
          <h2>{copy.developer.title}</h2>
          <p>{copy.developer.body}</p>
        </header>
        <div data-reveal>
          <DeveloperConsole copy={copy.developer} href={href} />
        </div>
      </section>

      <section
        className="flow-chapter flow-architecture-chapter"
        id="architecture"
      >
        <header className="flow-chapter__heading" data-reveal>
          <ChapterMeta chapter={copy.architecture.chapter} />
          <h2>{copy.architecture.title}</h2>
          <p>{copy.architecture.body}</p>
        </header>
        <div data-reveal>
          <ArchitectureMap copy={copy.architecture} />
        </div>
      </section>

      <section className="flow-final" data-reveal>
        <div>
          <Wrench aria-hidden="true" size={22} weight="duotone" />
          <span>{copy.final.eyebrow}</span>
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
