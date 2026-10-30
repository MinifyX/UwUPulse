import type { ReactNode } from 'react';

/** A page: title, one line under it, actions on the right, then the content. */
export function Page({
  title,
  subtitle,
  actions,
  children,
  wide,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <div className={wide ? 'page page-wide' : 'page'}>
      <header className="page-head">
        <div className="min-w-0">
          <h1 className="page-title">{title}</h1>
          {subtitle && <p className="page-subtitle">{subtitle}</p>}
        </div>
        {actions && <div className="page-actions">{actions}</div>}
      </header>
      {children}
    </div>
  );
}
