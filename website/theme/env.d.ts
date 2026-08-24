/// <reference types="@rspress/core/types" />

declare module '*.css';
declare module '@a3s-lab/ui/basecoat';
declare module '@a3s-lab/ui/code-editor';

type A3SCodeEditorElement = HTMLDivElement & {
  setValue?: (value: string, options?: { clean?: boolean }) => void;
};

interface A3SUIRuntime {
  init: (componentName: string) => void;
  refresh: (element: Element) => void;
  start: () => void;
}

interface Window {
  basecoat?: A3SUIRuntime;
}

interface ImportMetaEnv {
  readonly SSG_MD?: boolean;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
