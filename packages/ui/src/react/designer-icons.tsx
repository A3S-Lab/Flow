import type { SVGProps } from 'react';

export type DesignerIconName =
  | 'alert'
  | 'arrow-down'
  | 'arrow-up'
  | 'calculator'
  | 'calendar'
  | 'card'
  | 'check-square'
  | 'chevron-down'
  | 'close'
  | 'collapse'
  | 'columns-2'
  | 'columns-3'
  | 'currency'
  | 'components'
  | 'copy'
  | 'desktop'
  | 'edit'
  | 'email'
  | 'eye'
  | 'eye-off'
  | 'field'
  | 'file'
  | 'grip'
  | 'grid'
  | 'hash'
  | 'info'
  | 'layout'
  | 'list'
  | 'link'
  | 'lock'
  | 'mobile'
  | 'more'
  | 'multi-select'
  | 'phone'
  | 'play'
  | 'radio'
  | 'redo'
  | 'search'
  | 'settings'
  | 'spacer'
  | 'sparkles'
  | 'slider'
  | 'star'
  | 'signature'
  | 'tabs'
  | 'tag'
  | 'text'
  | 'textarea'
  | 'toggle'
  | 'trash'
  | 'undo';

export interface DesignerIconProps extends SVGProps<SVGSVGElement> {
  name: DesignerIconName;
  size?: number;
}

export function DesignerIcon({ name, size = 16, ...props }: DesignerIconProps) {
  return (
    <svg aria-hidden="true" fill="none" height={size} viewBox="0 0 24 24" width={size} {...props}>
      {name === 'alert' && (
        <>
          <path d="M12 3 2.8 20h18.4L12 3Z" stroke="currentColor" strokeLinejoin="round" />
          <path d="M12 9v5m0 3h.01" stroke="currentColor" strokeLinecap="round" strokeWidth="1.8" />
        </>
      )}
      {name === 'arrow-down' && (
        <path
          d="M12 5v14m0 0 5-5m-5 5-5-5"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.7"
        />
      )}
      {name === 'arrow-up' && (
        <path
          d="M12 19V5m0 0 5 5m-5-5-5 5"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.7"
        />
      )}
      {name === 'calculator' && (
        <>
          <rect
            x="4"
            y="3"
            width="16"
            height="18"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path
            d="M7 7h10v3H7V7Zm1 7h.01m4 0h.01m4 0h.01M8 18h.01m4 0h.01m4 0h.01"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="2"
          />
        </>
      )}
      {name === 'calendar' && (
        <>
          <rect
            x="4"
            y="5.5"
            width="16"
            height="14"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path
            d="M8 3.5v4M16 3.5v4M4 9.5h16M8 13h2m4 0h2m-8 3h2"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'card' && (
        <>
          <rect
            x="3.5"
            y="5"
            width="17"
            height="14"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path
            d="M7 9h7M7 13h10M7 16h6"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'check-square' && (
        <>
          <rect x="4" y="4" width="16" height="16" rx="3" stroke="currentColor" strokeWidth="1.6" />
          <path
            d="m8 12 2.6 2.7L16.5 9"
            stroke="currentColor"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="1.8"
          />
        </>
      )}
      {name === 'chevron-down' && (
        <path
          d="m6.5 9 5.5 5.5L17.5 9"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.8"
        />
      )}
      {name === 'close' && (
        <path
          d="m7 7 10 10M17 7 7 17"
          stroke="currentColor"
          strokeLinecap="round"
          strokeWidth="1.8"
        />
      )}
      {name === 'collapse' && (
        <>
          <rect
            x="4"
            y="4"
            width="16"
            height="6"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <rect
            x="4"
            y="14"
            width="16"
            height="6"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="m16.5 6.5-2 2-2-2"
            stroke="currentColor"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="1.3"
          />
        </>
      )}
      {name === 'columns-2' && (
        <>
          <rect
            x="3.5"
            y="4"
            width="17"
            height="16"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path d="M12 4v16" stroke="currentColor" strokeWidth="1.6" />
        </>
      )}
      {name === 'columns-3' && (
        <>
          <rect
            x="3.5"
            y="4"
            width="17"
            height="16"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path d="M9.2 4v16M14.8 4v16" stroke="currentColor" strokeWidth="1.5" />
        </>
      )}
      {name === 'currency' && (
        <>
          <circle cx="12" cy="12" r="8.5" stroke="currentColor" strokeWidth="1.6" />
          <path
            d="M8.5 7.5 12 12l3.5-4.5M12 12v5m-3.5-2.5h7"
            stroke="currentColor"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'components' && (
        <>
          <rect
            x="4"
            y="4"
            width="6.5"
            height="6.5"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <rect
            x="13.5"
            y="4"
            width="6.5"
            height="6.5"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <rect
            x="4"
            y="13.5"
            width="6.5"
            height="6.5"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="M13.5 16.75H20M16.75 13.5V20"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'copy' && (
        <>
          <rect x="8" y="8" width="11" height="11" rx="2" stroke="currentColor" strokeWidth="1.6" />
          <path
            d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"
            stroke="currentColor"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'desktop' && (
        <>
          <rect
            x="3.5"
            y="4.5"
            width="17"
            height="12"
            rx="2"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path
            d="M9 20h6m-3-3.5V20"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'edit' && (
        <path
          d="M5 19h4L19 9l-4-4L5 15v4Zm8.5-12.5 4 4"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.6"
        />
      )}
      {name === 'email' && (
        <>
          <rect
            x="3.5"
            y="5"
            width="17"
            height="14"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path
            d="m5 7 7 6 7-6"
            stroke="currentColor"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'eye' && (
        <>
          <path
            d="M2.8 12s3.2-6 9.2-6 9.2 6 9.2 6-3.2 6-9.2 6-9.2-6-9.2-6Z"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="1.6"
          />
          <circle cx="12" cy="12" r="2.5" stroke="currentColor" strokeWidth="1.6" />
        </>
      )}
      {name === 'field' && (
        <>
          <rect
            x="3.5"
            y="5"
            width="17"
            height="14"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="M7 9h4M7 13h10M7 16h7"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.5"
          />
        </>
      )}
      {name === 'file' && (
        <>
          <path
            d="M7 3.5h6.5L18.5 8v12.5H7z"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="1.6"
          />
          <path
            d="M13.5 3.5V8h5M10 12.5h5.5M10 16h4"
            stroke="currentColor"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'eye-off' && (
        <>
          <path
            d="M3 12s3.2-5 9-5c2 0 3.7.6 5 1.5M21 12s-3.2 5-9 5c-2 0-3.7-.6-5-1.5"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
          <path d="m4 4 16 16" stroke="currentColor" strokeLinecap="round" strokeWidth="1.7" />
        </>
      )}
      {name === 'grip' && (
        <>
          <circle cx="9" cy="7" r="1" fill="currentColor" />
          <circle cx="15" cy="7" r="1" fill="currentColor" />
          <circle cx="9" cy="12" r="1" fill="currentColor" />
          <circle cx="15" cy="12" r="1" fill="currentColor" />
          <circle cx="9" cy="17" r="1" fill="currentColor" />
          <circle cx="15" cy="17" r="1" fill="currentColor" />
        </>
      )}
      {name === 'grid' && (
        <>
          <rect
            x="4"
            y="4"
            width="16"
            height="16"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="M9.3 4v16M14.7 4v16M4 9.3h16M4 14.7h16"
            stroke="currentColor"
            strokeWidth="1.3"
          />
        </>
      )}
      {name === 'hash' && (
        <path
          d="M9 3 7 21m10-18-2 18M4 9h17M3 15h17"
          stroke="currentColor"
          strokeLinecap="round"
          strokeWidth="1.6"
        />
      )}
      {name === 'info' && (
        <>
          <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="1.6" />
          <path
            d="M12 10.5V17m0-10h.01"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.8"
          />
        </>
      )}
      {name === 'layout' && (
        <>
          <rect
            x="3.5"
            y="4"
            width="17"
            height="16"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path d="M3.5 9h17M9 9v11" stroke="currentColor" strokeWidth="1.5" />
        </>
      )}
      {name === 'list' && (
        <>
          <path
            d="M9 6h11M9 12h11M9 18h11"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
          <circle cx="5" cy="6" r="1" fill="currentColor" />
          <circle cx="5" cy="12" r="1" fill="currentColor" />
          <circle cx="5" cy="18" r="1" fill="currentColor" />
        </>
      )}
      {name === 'link' && (
        <>
          <path d="m9.5 14.5 5-5" stroke="currentColor" strokeLinecap="round" strokeWidth="1.7" />
          <path
            d="M7.8 16.2 6.6 17.4a3.5 3.5 0 0 1-5-5l3-3a3.5 3.5 0 0 1 5 0M16.2 7.8l1.2-1.2a3.5 3.5 0 0 1 5 5l-3 3a3.5 3.5 0 0 1-5 0"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
            transform="translate(-.1)"
          />
        </>
      )}
      {name === 'lock' && (
        <>
          <rect
            x="5"
            y="10"
            width="14"
            height="10"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path d="M8 10V7a4 4 0 0 1 8 0v3" stroke="currentColor" strokeWidth="1.6" />
        </>
      )}
      {name === 'mobile' && (
        <>
          <rect
            x="7"
            y="2.5"
            width="10"
            height="19"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.6"
          />
          <path
            d="M10.5 5h3M11 18.5h2"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.5"
          />
        </>
      )}
      {name === 'more' && (
        <path
          d="M5 12h.01M12 12h.01M19 12h.01"
          stroke="currentColor"
          strokeLinecap="round"
          strokeWidth="2.8"
        />
      )}
      {name === 'multi-select' && (
        <>
          <rect
            x="3.5"
            y="4"
            width="6"
            height="6"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="m5.3 7 1.2 1.2L8.4 6"
            stroke="currentColor"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="1.4"
          />
          <rect
            x="3.5"
            y="14"
            width="6"
            height="6"
            rx="1.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="m5.3 17 1.2 1.2 1.9-2.2M13 7h7M13 17h7"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
        </>
      )}
      {name === 'phone' && (
        <path
          d="M7.2 3.5 10 7.8 8.2 10c1.2 2.5 3.3 4.6 5.8 5.8l2.2-1.8 4.3 2.8-.8 3.2c-.2.7-.9 1.1-1.6 1-7.7-1-14-7.4-15.1-15.1-.1-.7.3-1.4 1-1.6l3.2-.8Z"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.6"
        />
      )}
      {name === 'play' && (
        <path
          d="m8 5 11 7-11 7V5Z"
          stroke="currentColor"
          strokeLinejoin="round"
          strokeWidth="1.7"
        />
      )}
      {name === 'radio' && (
        <>
          <circle cx="12" cy="12" r="8" stroke="currentColor" strokeWidth="1.6" />
          <circle cx="12" cy="12" r="3" fill="currentColor" />
        </>
      )}
      {name === 'redo' && (
        <path
          d="M20 8h-8a7 7 0 1 0 6.3 10M20 8l-4-4m4 4-4 4"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.7"
        />
      )}
      {name === 'search' && (
        <>
          <circle cx="10.5" cy="10.5" r="6" stroke="currentColor" strokeWidth="1.7" />
          <path d="m15 15 4.5 4.5" stroke="currentColor" strokeLinecap="round" strokeWidth="1.7" />
        </>
      )}
      {name === 'settings' && (
        <>
          <circle cx="12" cy="12" r="3" stroke="currentColor" strokeWidth="1.6" />
          <path
            d="M19.2 13.8 21 15l-2 3.5-2.1-.9a7.9 7.9 0 0 1-2.2 1.3l-.3 2.3h-4l-.3-2.3a7.9 7.9 0 0 1-2.2-1.3l-2.1.9-2-3.5 1.8-1.2a8.2 8.2 0 0 1 0-2.6L3.8 10l2-3.5 2.1.9a7.9 7.9 0 0 1 2.2-1.3l.3-2.3h4l.3 2.3a7.9 7.9 0 0 1 2.2 1.3l2.1-.9 2 3.5-1.8 1.2a8.2 8.2 0 0 1 0 2.6Z"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="1.35"
          />
        </>
      )}
      {name === 'spacer' && (
        <path
          d="M5 5h14M5 19h14M12 8v8m0-8-2 2m2-2 2 2m-2 6-2-2m2 2 2-2"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.5"
        />
      )}
      {name === 'sparkles' && (
        <path
          d="M12 2.8c.4 4 2.4 6 6.2 6.5-3.8.4-5.8 2.5-6.2 6.4-.4-3.9-2.4-6-6.2-6.4C9.6 8.8 11.6 6.8 12 2.8ZM18.5 15.5c.2 1.8 1.1 2.7 2.7 3-1.6.2-2.5 1.1-2.7 2.8-.2-1.7-1.1-2.6-2.7-2.8 1.6-.3 2.5-1.2 2.7-3Z"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.4"
        />
      )}
      {name === 'slider' && (
        <>
          <path
            d="M4 7h8m4 0h4M4 17h3m4 0h9"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.6"
          />
          <circle cx="14" cy="7" r="2" stroke="currentColor" strokeWidth="1.6" />
          <circle cx="9" cy="17" r="2" stroke="currentColor" strokeWidth="1.6" />
        </>
      )}
      {name === 'star' && (
        <path
          d="m12 3 2.7 5.5 6.1.9-4.4 4.3 1 6.1-5.4-2.9-5.4 2.9 1-6.1-4.4-4.3 6.1-.9L12 3Z"
          stroke="currentColor"
          strokeLinejoin="round"
          strokeWidth="1.6"
        />
      )}
      {name === 'signature' && (
        <>
          <path
            d="m5 16.8.9-3.7 8.8-8.8 4 4-8.8 8.8-3.7.9L5 16.8Z"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="1.5"
          />
          <path
            d="m13.2 5.8 4 4M4 20h16"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.5"
          />
        </>
      )}
      {name === 'tabs' && (
        <>
          <path
            d="M4 9h16v11H4V9Z"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="1.5"
          />
          <path
            d="M4 9V5h6l1.5 4M12 5h5l1.5 4"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="1.5"
          />
        </>
      )}
      {name === 'tag' && (
        <path
          d="M3.5 5v6.3L12.2 20l7.8-7.8L11.3 3.5H5A1.5 1.5 0 0 0 3.5 5Z"
          stroke="currentColor"
          strokeLinejoin="round"
          strokeWidth="1.6"
        />
      )}
      {name === 'text' && (
        <path
          d="M5 6V4h14v2M12 4v16m-4 0h8"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.7"
        />
      )}
      {name === 'textarea' && (
        <>
          <rect
            x="3.5"
            y="4"
            width="17"
            height="16"
            rx="2.5"
            stroke="currentColor"
            strokeWidth="1.5"
          />
          <path
            d="M7 8h10M7 12h10M7 16h6"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.5"
          />
        </>
      )}
      {name === 'toggle' && (
        <>
          <rect x="3" y="7" width="18" height="10" rx="5" stroke="currentColor" strokeWidth="1.6" />
          <circle cx="15.5" cy="12" r="3" fill="currentColor" />
        </>
      )}
      {name === 'trash' && (
        <>
          <path
            d="M5 7h14M9 7V4h6v3m2 0-1 13H8L7 7"
            stroke="currentColor"
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth="1.6"
          />
          <path
            d="M10 11v5M14 11v5"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="1.5"
          />
        </>
      )}
      {name === 'undo' && (
        <path
          d="M4 8h8a7 7 0 1 1-6.3 10M4 8l4-4M4 8l4 4"
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.7"
        />
      )}
    </svg>
  );
}

const catalogIconById: Readonly<Record<string, DesignerIconName>> = {
  text: 'text',
  textarea: 'textarea',
  number: 'hash',
  email: 'email',
  password: 'lock',
  date: 'calendar',
  url: 'link',
  tel: 'phone',
  'date-time': 'calendar',
  time: 'calendar',
  select: 'chevron-down',
  radio: 'radio',
  checkbox: 'check-square',
  switch: 'toggle',
  'multi-select': 'multi-select',
  'matrix-single': 'grid',
  'matrix-multiple': 'grid',
  tags: 'tag',
  'data-grid': 'grid',
  repeater: 'list',
  'repeater-group': 'list',
  currency: 'currency',
  rating: 'star',
  slider: 'slider',
  hidden: 'eye-off',
  calculated: 'calculator',
  grid: 'grid',
  'columns-2': 'columns-2',
  'columns-3': 'columns-3',
  card: 'card',
  wizard: 'list',
  tabs: 'tabs',
  collapse: 'collapse',
  content: 'info',
  divider: 'layout',
  spacer: 'spacer',
  'custom:a3s.file-upload': 'file',
  'custom:a3s.signature': 'signature',
};

export function CatalogIcon({ id, fallback }: { id: string; fallback: string }) {
  const name = catalogIconById[id];
  return name ? <DesignerIcon name={name} size={16} /> : <span>{fallback}</span>;
}
