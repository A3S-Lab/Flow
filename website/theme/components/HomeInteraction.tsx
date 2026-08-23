import { useEffect, useRef } from 'react';

const MOTION_SELECTOR = '.flow-motion-scene';

export function HomeInteraction() {
  const anchorRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const host = anchorRef.current?.closest<HTMLElement>('.flow-home');
    if (!host) return undefined;

    const motionPreference = window.matchMedia(
      '(prefers-reduced-motion: reduce)',
    );
    const visibleMotionItems = new Set<HTMLElement>();
    const motionItems = [
      ...host.querySelectorAll<HTMLElement>(MOTION_SELECTOR),
    ];

    const syncMotion = () => {
      const shouldRun = !motionPreference.matches && !document.hidden;
      for (const item of motionItems) {
        item.classList.toggle(
          'is-motion-active',
          shouldRun && visibleMotionItems.has(item),
        );
      }
    };

    const motionObserver =
      'IntersectionObserver' in window
        ? new IntersectionObserver(
            (entries) => {
              for (const entry of entries) {
                const item = entry.target as HTMLElement;
                if (entry.isIntersecting) visibleMotionItems.add(item);
                else visibleMotionItems.delete(item);
              }
              syncMotion();
            },
            { rootMargin: '120px 0px', threshold: 0.05 },
          )
        : undefined;

    for (const item of motionItems) {
      if (motionObserver) motionObserver.observe(item);
      else visibleMotionItems.add(item);
    }

    const revealItems = [
      ...host.querySelectorAll<HTMLElement>('[data-reveal]'),
    ];
    const revealObserver =
      !motionPreference.matches && 'IntersectionObserver' in window
        ? new IntersectionObserver(
            (entries) => {
              for (const entry of entries) {
                if (!entry.isIntersecting) continue;
                (entry.target as HTMLElement).classList.add('is-visible');
                revealObserver?.unobserve(entry.target);
              }
            },
            { rootMargin: '0px 0px -8% 0px', threshold: 0.08 },
          )
        : undefined;

    host.dataset.effects = 'ready';
    for (const item of revealItems) {
      if (revealObserver) revealObserver.observe(item);
      else item.classList.add('is-visible');
    }

    const handleMotionPreference = () => syncMotion();
    document.addEventListener('visibilitychange', syncMotion);
    motionPreference.addEventListener('change', handleMotionPreference);
    syncMotion();

    return () => {
      revealObserver?.disconnect();
      motionObserver?.disconnect();
      for (const item of motionItems) item.classList.remove('is-motion-active');
      delete host.dataset.effects;
      document.removeEventListener('visibilitychange', syncMotion);
      motionPreference.removeEventListener('change', handleMotionPreference);
    };
  }, []);

  return (
    <span aria-hidden="true" className="flow-effects-anchor" ref={anchorRef} />
  );
}
