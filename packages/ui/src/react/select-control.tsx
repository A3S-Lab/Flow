import type { SelectHTMLAttributes } from 'react';

/**
 * Temporary bridge for workflow-specific composite widgets. Ordinary form
 * fields render through A3S UI NativeWidget. Remove this bridge when
 * A3S-Lab/UI#10 publishes SelectControl from the supported React entry point.
 */
export function SelectControl({
  className,
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  const classes = ['select', className].filter(Boolean).join(' ');
  return (
    <span className="a3s-form-select-control">
      <select {...props} className={classes}>
        {children}
      </select>
    </span>
  );
}
