import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { vi } from 'vitest';

const runtimeGate = vi.hoisted(() => {
  let resolveGate: () => void = () => undefined;
  let promise: Promise<void> = Promise.resolve();

  return {
    reset() {
      promise = new Promise<void>((resolve) => {
        resolveGate = resolve;
      });
    },
    resolve() {
      resolveGate();
    },
    get promise() {
      return promise;
    },
  };
});

vi.mock('../src/react/a3s-ui-runtime', () => ({
  loadA3SUIRuntime: () => runtimeGate.promise,
}));

import { SelectControl } from '../src/react/select-control';

describe('SelectControl runtime fallback', () => {
  beforeEach(() => {
    runtimeGate.reset();
  });

  afterEach(() => {
    runtimeGate.resolve();
  });

  it('opens and selects on the first click while the runtime is loading', () => {
    const onChange = vi.fn();
    const view = render(
      <SelectControl aria-label="Run mode" onChange={onChange} value="durable">
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    const trigger = screen.getByRole('combobox', { name: 'Run mode' });

    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');

    fireEvent.click(screen.getByRole('option', { name: 'Local' }));
    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ target: { value: 'local' } }),
    );
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    view.unmount();
  });

  it('supports keyboard selection before the runtime is ready', () => {
    const onChange = vi.fn();
    const view = render(
      <SelectControl
        aria-label="Keyboard mode"
        onChange={onChange}
        value="durable"
      >
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    const trigger = screen.getByRole('combobox', { name: 'Keyboard mode' });

    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    expect(trigger.getAttribute('aria-expanded')).toBe('true');
    fireEvent.keyDown(trigger, { key: 'ArrowDown' });
    expect(trigger.getAttribute('aria-activedescendant')).toContain('option-2');
    fireEvent.keyDown(trigger, { key: 'Enter' });
    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ target: { value: 'local' } }),
    );
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    view.unmount();
  });

  it('hands an open fallback popover to the runtime without toggling twice', async () => {
    const view = render(
      <SelectControl aria-label="Runtime handoff" value="durable">
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    const root = view.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    ) as HTMLElement & {
      close?: (focusOnTrigger?: boolean) => void;
      open?: () => void;
      refresh?: () => void;
      value?: string;
    };
    const trigger = screen.getByRole('combobox', {
      name: 'Runtime handoff',
    });
    const popover = root.querySelector<HTMLElement>('[data-popover]');
    expect(popover).not.toBeNull();

    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');

    const runtimeOpen = vi.fn(() => {
      popover?.setAttribute('aria-hidden', 'false');
      trigger.setAttribute('aria-expanded', 'true');
    });
    const runtimeClose = vi.fn(() => {
      popover?.setAttribute('aria-hidden', 'true');
      trigger.setAttribute('aria-expanded', 'false');
    });
    const runtimeToggle = vi.fn(() => {
      if (trigger.getAttribute('aria-expanded') === 'true') runtimeClose();
      else runtimeOpen();
      return true;
    });
    root.open = runtimeOpen;
    root.close = runtimeClose;
    root.refresh = vi.fn();
    root.togglePopover = () => runtimeToggle();
    const runtimeListener = () => root.togglePopover?.();
    trigger.addEventListener('click', runtimeListener);

    runtimeGate.resolve();
    await waitFor(() => expect(runtimeOpen).toHaveBeenCalledTimes(1));
    expect(trigger.getAttribute('aria-expanded')).toBe('true');

    // The runtime's direct listener owns this click. React should only mirror
    // the resulting state and must not invoke a second toggle.
    fireEvent.click(trigger);
    expect(runtimeToggle).toHaveBeenCalledTimes(1);
    expect(runtimeOpen).toHaveBeenCalledTimes(1);
    expect(runtimeClose).toHaveBeenCalledTimes(1);
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    trigger.removeEventListener('click', runtimeListener);
    view.unmount();
  });

  it('does not reopen a fallback popover when the control becomes disabled', async () => {
    const view = render(
      <SelectControl aria-label="Disable during handoff" value="durable">
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    const root = view.container.querySelector<HTMLElement>(
      '.a3s-flow-select-control',
    ) as HTMLElement & {
      close?: (focusOnTrigger?: boolean) => void;
      open?: () => void;
      refresh?: () => void;
      value?: string;
    };
    const trigger = screen.getByRole('combobox', {
      name: 'Disable during handoff',
    });
    fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');

    const runtimeOpen = vi.fn(() => {
      trigger.setAttribute('aria-expanded', 'true');
    });
    const popover = root.querySelector<HTMLElement>('[data-popover]');
    const runtimeClose = vi.fn(() => {
      popover?.setAttribute('aria-hidden', 'true');
      trigger.setAttribute('aria-expanded', 'false');
    });
    root.open = runtimeOpen;
    root.close = runtimeClose;
    root.refresh = vi.fn();
    root.togglePopover = () => true;

    view.rerender(
      <SelectControl
        aria-label="Disable during handoff"
        disabled
        value="durable"
      >
        <option value="durable">Durable</option>
        <option value="local">Local</option>
      </SelectControl>,
    );
    runtimeGate.resolve();
    await waitFor(() =>
      expect((trigger as HTMLButtonElement).disabled).toBe(true),
    );
    await Promise.resolve();

    expect(runtimeOpen).not.toHaveBeenCalled();
    expect(runtimeClose).toHaveBeenCalled();
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
    view.unmount();
  });
});
