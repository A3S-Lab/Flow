import { withBase } from '@rspress/core/runtime';
import type { FlowWebsiteLocale } from './flow-node-catalog';

export function pageHref(
  route: string,
  locale: FlowWebsiteLocale,
  version: string,
  defaultVersion: string,
): string {
  const prefix = [
    version !== defaultVersion ? version : '',
    locale === 'en' ? 'en' : '',
  ]
    .filter(Boolean)
    .join('/');
  return withBase(
    `/${[prefix, route.replace(/^\//u, '')].filter(Boolean).join('/')}`,
  );
}

export function playgroundHref(
  locale: FlowWebsiteLocale,
  version: string,
  defaultVersion: string,
  exampleId?: string,
): string {
  const href = pageHref('playground', locale, version, defaultVersion);
  return exampleId ? `${href}?example=${encodeURIComponent(exampleId)}` : href;
}
