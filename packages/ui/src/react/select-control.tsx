import type { SelectHTMLAttributes } from 'react';

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
