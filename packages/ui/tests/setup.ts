// The shared A3S UI React adapters load the browser-wide runtime. jsdom does
// not provide matchMedia, which the runtime uses while registering responsive
// navigation behavior. Keep the test environment browser-shaped without
// changing production code paths.
if (typeof window !== 'undefined' && typeof window.matchMedia !== 'function') {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => false,
    }),
  });
}

// jsdom's selector engine does not implement the CSS :user-invalid pseudo
// class used by the A3S accessibility scanner. Preserve the scanner's other
// matching behavior while treating that browser-only pseudo as a no-op in
// tests.
if (typeof Element !== 'undefined') {
  const nativeMatches = Element.prototype.matches;
  const compatibleMatches = function matches(
    this: Element,
    selector: string,
  ): boolean {
    try {
      return nativeMatches.call(this, selector);
    } catch (error) {
      if (!selector.includes(':user-invalid')) throw error;
      const compatibleSelector = selector
        .split(',')
        .filter((part) => !part.includes(':user-invalid'))
        .join(',');
      return compatibleSelector.trim()
        ? nativeMatches.call(this, compatibleSelector)
        : false;
    }
  } as unknown as typeof nativeMatches;
  Element.prototype.matches = compatibleMatches;
}
