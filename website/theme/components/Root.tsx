import { useContext, useEffect, type ReactNode } from 'react';
import { ThemeContext } from '@rspress/core/runtime';

type RootProps = {
  children: ReactNode;
};

export function Root({ children }: RootProps) {
  const { theme } = useContext(ThemeContext);

  useEffect(() => {
    document
      .querySelector<HTMLMetaElement>('meta[name="theme-color"]')
      ?.setAttribute('content', theme === 'dark' ? '#111112' : '#f6f6f6');
  }, [theme]);

  return children;
}
