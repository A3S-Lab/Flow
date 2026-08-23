import { useLocation } from '@rspress/core/runtime';
import {
  Layout as OriginalLayout,
  type LayoutProps,
} from '@rspress/core/theme-original';
import { useEffect } from 'react';

const FOCUSABLE_SELECTOR = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[contenteditable="true"]',
  '[tabindex]',
].join(',');

const PREVIOUS_TABINDEX = 'data-flow-previous-tabindex';
const NO_TABINDEX = '__none__';

function setSidebarAvailable(sidebar: HTMLElement, available: boolean) {
  sidebar.toggleAttribute('inert', !available);
  if (available) {
    sidebar.removeAttribute('aria-hidden');
  } else {
    sidebar.setAttribute('aria-hidden', 'true');
  }

  sidebar
    .querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)
    .forEach((element) => {
      if (!available) {
        if (!element.hasAttribute(PREVIOUS_TABINDEX)) {
          element.setAttribute(
            PREVIOUS_TABINDEX,
            element.getAttribute('tabindex') ?? NO_TABINDEX,
          );
        }
        element.tabIndex = -1;
        return;
      }

      const previous = element.getAttribute(PREVIOUS_TABINDEX);
      if (previous === null) return;
      if (previous === NO_TABINDEX) {
        element.removeAttribute('tabindex');
      } else {
        element.setAttribute('tabindex', previous);
      }
      element.removeAttribute(PREVIOUS_TABINDEX);
    });
}

function useAccessibleMobileSidebar() {
  const { pathname } = useLocation();

  useEffect(() => {
    const mobile = window.matchMedia('(max-width: 768px)');
    const sync = () => {
      document
        .querySelectorAll<HTMLElement>('.rp-doc-layout__sidebar')
        .forEach((sidebar) => {
          const open = sidebar.classList.contains(
            'rp-doc-layout__sidebar--open',
          );
          setSidebarAvailable(sidebar, !mobile.matches || open);
        });
    };

    const observer = new MutationObserver(sync);
    observer.observe(document.body, {
      attributeFilter: ['class'],
      attributes: true,
      childList: true,
      subtree: true,
    });
    mobile.addEventListener('change', sync);
    sync();

    return () => {
      observer.disconnect();
      mobile.removeEventListener('change', sync);
      document
        .querySelectorAll<HTMLElement>('.rp-doc-layout__sidebar')
        .forEach((sidebar) => setSidebarAvailable(sidebar, true));
    };
  }, [pathname]);
}

export function Layout(props: LayoutProps) {
  useAccessibleMobileSidebar();
  return <OriginalLayout {...props} />;
}
