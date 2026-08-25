const runtimePromises = new WeakMap<Window, Map<string, Promise<void>>>();

function isCurrentWindow(owner: Window): boolean {
  return typeof window !== 'undefined' && window === owner;
}

/**
 * Loads one A3S UI custom-element runtime for the current browser window.
 * The captured window keeps late dynamic imports safe when a test or host
 * tears down the document before the import settles.
 */
export function loadA3SUIRuntime(
  componentName: string,
  loadComponent: () => Promise<unknown>,
): Promise<void> {
  if (typeof window === 'undefined') return Promise.resolve();

  const owner = window;
  let promises = runtimePromises.get(owner);
  if (!promises) {
    promises = new Map();
    runtimePromises.set(owner, promises);
  }

  const existing = promises.get(componentName);
  if (existing) return existing;

  const runtime = (async () => {
    if (!owner.basecoat) await import('@a3s-lab/ui/basecoat');
    if (!isCurrentWindow(owner)) return;

    await loadComponent();
    if (!isCurrentWindow(owner)) return;

    owner.basecoat?.init(componentName);
    owner.basecoat?.start();
  })();
  promises.set(componentName, runtime);
  void runtime.catch(() => {
    if (promises?.get(componentName) === runtime) {
      promises.delete(componentName);
    }
  });
  return runtime;
}
