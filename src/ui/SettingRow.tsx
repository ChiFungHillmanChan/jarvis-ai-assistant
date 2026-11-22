import type { ReactNode } from 'react';
import './ui.css';

interface SettingRowProps {
  id?: string;
  title: string;
  description?: string;
  badge?: string;
  control: ReactNode;
  children?: ReactNode;
}

export default function SettingRow({ id, title, description, badge, control, children }: SettingRowProps) {
  return (
    <div className="ui-setting-row" id={id}>
      <div className="ui-setting-text">
        <div className="ui-setting-title">
          <span>{title}</span>
          {badge && <span className="ui-setting-badge">{badge}</span>}
        </div>
        {description && <div className="ui-setting-desc">{description}</div>}
        {children && <div className="ui-setting-extra">{children}</div>}
      </div>
      <div className="ui-setting-control">{control}</div>
    </div>
  );
}
