import {
  a3sFlowDagNodeRegistry,
  localizeA3SFlowDagManifest,
  type A3SFlowDagNodeCatalog,
} from '@a3s-lab/flow-ui';
import { useLang, useSite, useVersion, withBase } from '@rspress/core/runtime';
import { ReactFlowProvider } from '@xyflow/react';
import { useMemo, useSyncExternalStore, type ComponentType } from 'react';
import { workflowPlaygroundCopy } from './WorkflowPlayground.copy';
import {
  createWorkflowExamples,
  findWorkflowExample,
  type WorkflowExampleDefinition,
} from './WorkflowPlayground.examples';
import { workflowExamplesCopy } from './WorkflowPlayground.examples.copy';
import { WorkflowPlaygroundExamples } from './WorkflowPlaygroundExamples';
import { createPlaygroundNodeCatalog } from './WorkflowPlayground.custom-nodes';
import type {
  WorkflowPlaygroundCopilotRequest,
  WorkflowPlaygroundExtensionSlots,
} from './WorkflowPlayground.extensions';
import { pageHref, playgroundHref } from './WorkflowPlayground.routes';
import { flowNodeGroups, type FlowWebsiteLocale } from './flow-node-catalog';

const PLAYGROUND_ROUTE_EVENT = 'a3s-flow-playground-route';

function browserSearchSnapshot(): string {
  return typeof window === 'undefined' ? '' : window.location.search;
}

function subscribeToPlaygroundRoute(onStoreChange: () => void): () => void {
  if (typeof window === 'undefined') return () => {};
  window.addEventListener('popstate', onStoreChange);
  window.addEventListener(PLAYGROUND_ROUTE_EVENT, onStoreChange);
  return () => {
    window.removeEventListener('popstate', onStoreChange);
    window.removeEventListener(PLAYGROUND_ROUTE_EVENT, onStoreChange);
  };
}

function usePlaygroundSearch(): string {
  return useSyncExternalStore(
    subscribeToPlaygroundRoute,
    browserSearchSnapshot,
    () => '',
  );
}

function navigatePlayground(href: string) {
  window.history.pushState(null, '', href);
  window.dispatchEvent(new Event(PLAYGROUND_ROUTE_EVENT));
  window.scrollTo({ top: 0, behavior: 'auto' });
}

function MarkdownPlayground({
  examples,
  locale,
}: {
  examples: readonly WorkflowExampleDefinition[];
  locale: FlowWebsiteLocale;
}) {
  const copy = workflowPlaygroundCopy[locale];
  const examplesCopy = workflowExamplesCopy[locale];
  return (
    <main data-flow-playground="">
      <h1>{examplesCopy.pageTitle}</h1>
      <p>{examplesCopy.pageDescription}</p>
      <ul>
        {examples.map((example) => (
          <li key={example.id}>
            <strong>{example.title}</strong>: {example.description}
          </li>
        ))}
      </ul>
      <h2>{copy.nodeLibrary}</h2>
      <p>{copy.nodeLibraryDescription}</p>
      <ul>
        {flowNodeGroups.flatMap((group) =>
          group.types.map((type) => {
            const node = localizeA3SFlowDagManifest(
              a3sFlowDagNodeRegistry.require(type),
              locale,
            );
            return <li key={type}>{node.display_name}</li>;
          }),
        )}
      </ul>
    </main>
  );
}

export type WorkflowPlaygroundSurfaceProps = {
  backHref: string;
  catalog: A3SFlowDagNodeCatalog;
  example: WorkflowExampleDefinition;
  extensions?: WorkflowPlaygroundExtensionSlots;
  onCopilotRequest?: (
    request: WorkflowPlaygroundCopilotRequest,
  ) => void | Promise<void>;
};

type WorkflowPlaygroundRouteProps = {
  surface: ComponentType<WorkflowPlaygroundSurfaceProps>;
  extensions?: WorkflowPlaygroundExtensionSlots;
  onCopilotRequest?: (
    request: WorkflowPlaygroundCopilotRequest,
  ) => void | Promise<void>;
};

export function WorkflowPlaygroundRoute({
  surface: Surface,
  extensions,
  onCopilotRequest,
}: WorkflowPlaygroundRouteProps) {
  const locale: FlowWebsiteLocale = useLang() === 'en' ? 'en' : 'zh';
  const catalog = useMemo(
    () => createPlaygroundNodeCatalog(locale, { includeDify: true }),
    [locale],
  );
  const examples = useMemo(
    () => createWorkflowExamples(locale, catalog),
    [catalog, locale],
  );
  const version = useVersion();
  const { site } = useSite();
  const defaultVersion = site.multiVersion.default ?? version;
  const versions = site.multiVersion.versions ?? [version];
  const search = usePlaygroundSearch();
  const requestedExampleId = new URLSearchParams(search).get('example');
  const selectedExample = findWorkflowExample(examples, requestedExampleId);
  const examplesHref = playgroundHref(locale, version, defaultVersion);

  if (import.meta.env.SSG_MD) {
    return <MarkdownPlayground examples={examples} locale={locale} />;
  }

  if (!selectedExample) {
    return (
      <WorkflowPlaygroundExamples
        exampleHref={(exampleId) =>
          playgroundHref(locale, version, defaultVersion, exampleId)
        }
        examples={examples}
        homeHref={pageHref('/', locale, version, defaultVersion)}
        languageHref={playgroundHref(
          locale === 'zh' ? 'en' : 'zh',
          version,
          defaultVersion,
        )}
        locale={locale}
        logoSrc={withBase('/a3s-logo.png')}
        onSelect={(exampleId) =>
          navigatePlayground(
            playgroundHref(locale, version, defaultVersion, exampleId),
          )
        }
        onVersionChange={(targetVersion) => {
          const target =
            targetVersion === defaultVersion
              ? playgroundHref(locale, targetVersion, defaultVersion)
              : pageHref('/', locale, targetVersion, defaultVersion);
          window.location.assign(target);
        }}
        unknownExampleId={requestedExampleId ?? undefined}
        version={version}
        versionHref={(targetVersion) =>
          targetVersion === defaultVersion
            ? playgroundHref(locale, targetVersion, defaultVersion)
            : pageHref('/', locale, targetVersion, defaultVersion)
        }
        versions={versions}
      />
    );
  }

  return (
    <ReactFlowProvider key={`${locale}:${selectedExample.id}`}>
      <Surface
        backHref={examplesHref}
        catalog={catalog}
        example={selectedExample}
        extensions={extensions}
        onCopilotRequest={onCopilotRequest}
      />
    </ReactFlowProvider>
  );
}
