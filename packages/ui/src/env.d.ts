declare module '@a3s-lab/ui/basecoat';
declare module '@a3s-lab/ui/code-editor';
declare module '@a3s-lab/ui/select';

interface Window {
  basecoat?: {
    init: (name?: string) => void;
    refresh: (element: HTMLElement) => void;
    start: () => void;
  };
}
