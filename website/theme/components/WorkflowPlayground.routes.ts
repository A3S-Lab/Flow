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
