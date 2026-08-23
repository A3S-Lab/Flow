import {
  removeBase,
  useLang,
  useLocation,
  useNav,
  usePage,
  useSite,
  useVersion,
} from '@rspress/core/runtime';
import {
  IconSmallMenu,
  NavTitle,
  Search,
  SocialLinks,
  SvgWrapper,
  type NavProps,
} from '@rspress/core/theme-original';
import {
  NavLangs,
  NavMenu,
  NavMenuDivider,
  NavMenuItemWithChildren,
} from '@rspress/core/dist/theme/components/Nav/NavMenu.js';
import { NavScreen } from '@rspress/core/dist/theme/components/NavScreen/index.js';
import { useNavScreen } from '@rspress/core/dist/theme/components/NavHamburger/useNavScreen.js';
import '@rspress/core/dist/theme/components/Nav/index.css';
import '@rspress/core/dist/theme/components/NavHamburger/index.css';
import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';

const CURRENT_ONLY_ROUTES = new Set([
  'concepts/child-workflows',
  'concepts/durable-primitives',
  'concepts/workflow-dag',
  'guide/cancellation',
  'guide/retries-and-waits',
  'guide/runtime-contract',
  'guide/signals-and-hooks',
  'nodes',
  'nodes/composition',
  'nodes/suspension',
  'nodes/terminal',
  'nodes/work',
  'operations/production',
  'operations/upgrading-to-v1',
  'reference/agent-skill',
  'reference/cli',
  'reference/examples',
  'runtimes/native-typescript',
]);

function labelSocialLinks(root: ParentNode, language: string) {
  root
    .querySelectorAll<HTMLAnchorElement>(
      '.rp-social-links__item[href="https://github.com/A3S-Lab/Flow"]',
    )
    .forEach((link) => {
      link.setAttribute(
        'aria-label',
        language === 'zh'
          ? '在 GitHub 上查看 A3S Flow'
          : 'View A3S Flow on GitHub',
      );
    });
}

function versionHref(
  pathname: string,
  currentVersion: string,
  targetVersion: string,
  defaultVersion: string,
  cleanUrls: boolean,
) {
  const parts = removeBase(pathname).split('/').filter(Boolean);

  if (currentVersion !== defaultVersion && parts[0] === currentVersion) {
    parts.shift();
  }
  const locale = parts[0] === 'en' ? parts[0] : '';
  const contentParts = locale ? parts.slice(1) : parts;
  const contentRoute = contentParts
    .join('/')
    .replace(/\.html$/, '')
    .replace(/\/index$/, '');

  if (
    currentVersion === defaultVersion &&
    targetVersion !== defaultVersion &&
    CURRENT_ONLY_ROUTES.has(contentRoute)
  ) {
    return `/${[targetVersion, locale].filter(Boolean).join('/')}/`;
  }
  if (targetVersion !== defaultVersion) {
    parts.unshift(targetVersion);
  }
  if (parts.length === 0) return '/';
  if (parts.length === 1 && targetVersion !== defaultVersion) {
    parts.push(cleanUrls ? 'index' : 'index.html');
  }
  return `/${parts.join('/')}`;
}

function NavVersions() {
  const { pathname } = useLocation();
  const { page } = usePage();
  const { site } = useSite();
  const currentVersion = useVersion();
  const defaultVersion = site.multiVersion.default ?? '';
  const versions = site.multiVersion.versions ?? [];
  const items = versions.map((version) => ({
    text: version,
    link: versionHref(
      page.pageType === '404' ? '/' : pathname,
      currentVersion,
      version,
      defaultVersion,
      site.route?.cleanUrls ?? false,
    ),
  }));

  return items.length > 1 ? (
    <NavMenuItemWithChildren
      activeMatcher={(item) => item.text === currentVersion}
      menuItem={{ text: currentVersion, items }}
    />
  ) : null;
}

function isVisibleControl(element: HTMLElement, screen: HTMLElement) {
  const clippedGroup = element.closest<HTMLElement>(
    '.rp-nav-screen-menu-item__group, .rp-nav-screen-langs-group, .rp-nav-screen-versions-group',
  );
  if (clippedGroup && clippedGroup.getBoundingClientRect().height < 2) {
    return false;
  }

  const style = window.getComputedStyle(element);
  const bounds = element.getBoundingClientRect();
  return (
    screen.contains(element) &&
    style.display !== 'none' &&
    style.visibility !== 'hidden' &&
    bounds.width > 0 &&
    bounds.height > 0
  );
}

function NavHamburger() {
  const { closeScreen, isScreenOpen, toggleScreen } = useNavScreen();
  const language = useLang();
  const trigger = useRef<HTMLButtonElement>(null);
  const closeScreenRef = useRef(closeScreen);
  closeScreenRef.current = closeScreen;
  const openLabel = language === 'zh' ? '打开导航' : 'Open navigation';
  const closeLabel = language === 'zh' ? '关闭导航' : 'Close navigation';

  useEffect(() => {
    if (!isScreenOpen) return;

    let dispose = () => {};
    const frame = window.requestAnimationFrame(() => {
      const screen = document.querySelector<HTMLElement>(
        '.rp-nav-screen--open',
      );
      if (!screen) return;

      screen.id = 'flow-mobile-navigation';
      screen.setAttribute(
        'aria-label',
        language === 'zh' ? '站点导航' : 'Site navigation',
      );
      screen.setAttribute('aria-modal', 'true');
      screen.setAttribute('role', 'dialog');

      const enhancedControls = new Map<HTMLElement, () => void>();
      let controlIndex = 0;
      const enhanceControls = () => {
        labelSocialLinks(screen, language);
        const controls = screen.querySelectorAll<HTMLElement>(
          '.rp-nav-screen-menu-item:not(a), .rp-nav-screen-langs, .rp-nav-screen-versions',
        );

        controls.forEach((control) => {
          if (enhancedControls.has(control)) return;
          const cleanups: Array<() => void> = [];
          const group = control.nextElementSibling as HTMLElement | null;
          const groupId = `flow-mobile-navigation-group-${controlIndex++}`;
          control.setAttribute('role', 'button');
          control.tabIndex = 0;
          if (group) {
            group.id = groupId;
            control.setAttribute('aria-controls', groupId);
          }

          const syncExpanded = () => {
            const expanded =
              control.classList.contains('rp-nav-screen-menu-item--open') ||
              control.querySelector('[class*="__icon--open"]') !== null ||
              group?.classList.contains('rp-nav-screen-versions-group--open') ||
              group?.style.gridTemplateRows === '1fr';
            control.setAttribute('aria-expanded', String(Boolean(expanded)));
            group?.setAttribute('aria-hidden', String(!expanded));
            group?.toggleAttribute('inert', !expanded);
            group
              ?.querySelectorAll<HTMLAnchorElement>('a[href]')
              .forEach((link) => {
                link.tabIndex = expanded ? 0 : -1;
              });
          };
          syncExpanded();

          const syncAfterClick = () =>
            window.requestAnimationFrame(syncExpanded);
          control.addEventListener('click', syncAfterClick);
          cleanups.push(() => {
            control.removeEventListener('click', syncAfterClick);
            group?.removeAttribute('aria-hidden');
            group?.removeAttribute('inert');
            group
              ?.querySelectorAll<HTMLAnchorElement>('a[href]')
              .forEach((link) => link.removeAttribute('tabindex'));
          });

          const activateWithKeyboard = (event: KeyboardEvent) => {
            if (event.key !== 'Enter' && event.key !== ' ') return;
            event.preventDefault();
            control.click();
          };
          control.addEventListener('keydown', activateWithKeyboard);
          cleanups.push(() =>
            control.removeEventListener('keydown', activateWithKeyboard),
          );
          enhancedControls.set(control, () =>
            cleanups.forEach((cleanup) => cleanup()),
          );
        });
      };

      enhanceControls();
      const observer = new MutationObserver(enhanceControls);
      observer.observe(screen, { childList: true, subtree: true });

      const focusableControls = () =>
        Array.from(
          screen.querySelectorAll<HTMLElement>(
            'a[href], button:not([disabled]), [tabindex="0"]',
          ),
        ).filter((element) => isVisibleControl(element, screen));

      const handleKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          event.preventDefault();
          closeScreenRef.current();
          return;
        }
        if (event.key !== 'Tab') return;

        const focusable = focusableControls();
        if (focusable.length === 0) return;
        const first = focusable[0];
        const last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      };

      screen.addEventListener('keydown', handleKeyDown);
      focusableControls()[0]?.focus();
      dispose = () => {
        observer.disconnect();
        screen.removeEventListener('keydown', handleKeyDown);
        enhancedControls.forEach((cleanup) => cleanup());
      };
    });

    return () => {
      window.cancelAnimationFrame(frame);
      dispose();
      if (trigger.current && document.contains(trigger.current)) {
        trigger.current.focus();
      }
    };
  }, [isScreenOpen, language]);

  return (
    <>
      {isScreenOpen &&
        createPortal(
          <NavScreen isScreenOpen={isScreenOpen} toggleScreen={toggleScreen} />,
          document.getElementById('__rspress_modal_container')!,
        )}
      <button
        aria-controls="flow-mobile-navigation"
        aria-expanded={isScreenOpen}
        aria-label={isScreenOpen ? closeLabel : openLabel}
        className={`rp-nav-hamburger rp-nav-hamburger__sm rp-nav-hamburger__md${isScreenOpen ? ' rp-nav-hamburger--active' : ''}`}
        onClick={toggleScreen}
        ref={trigger}
        type="button"
      >
        <SvgWrapper icon={IconSmallMenu} />
      </button>
    </>
  );
}

export function Nav({
  beforeNavTitle,
  afterNavTitle,
  beforeNavMenu,
  afterNavMenu,
  navTitle,
}: NavProps) {
  const navList = useNav();
  const language = useLang();

  useEffect(() => {
    labelSocialLinks(document, language);
  }, [language]);

  useEffect(() => {
    const mobileSearch = document.querySelector<HTMLElement>(
      '.rp-search-button--mobile',
    );
    if (!mobileSearch) return;

    mobileSearch.setAttribute(
      'aria-label',
      language === 'zh' ? '搜索文档' : 'Search documentation',
    );
    mobileSearch.setAttribute('role', 'button');
    mobileSearch.tabIndex = 0;

    const activateWithKeyboard = (event: KeyboardEvent) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      event.preventDefault();
      event.stopPropagation();
      mobileSearch.click();
    };
    mobileSearch.addEventListener('keydown', activateWithKeyboard);
    return () =>
      mobileSearch.removeEventListener('keydown', activateWithKeyboard);
  }, [language]);

  return (
    <header className="rp-nav">
      <div className="rp-nav__left">
        {beforeNavTitle}
        {navTitle ?? <NavTitle />}
        <NavMenu menuItems={navList} position="left" />
        {afterNavTitle}
      </div>
      <div className="rp-nav__right">
        {beforeNavMenu}
        <Search />
        <NavMenu menuItems={navList} position="right" />
        <div className="rp-nav__others">
          <NavMenuDivider />
          <NavLangs />
          <NavVersions />
          <SocialLinks />
        </div>
        <NavHamburger />
        {afterNavMenu}
      </div>
    </header>
  );
}
