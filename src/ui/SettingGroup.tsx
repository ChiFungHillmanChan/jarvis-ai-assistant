import { useId, type ReactNode } from 'react';
import './ui.css';

export default function SettingGroup({ title, children }: { title: string; children: ReactNode }) {
  const titleId = useId();
  return (
    <section className="ui-setting-group" aria-labelledby={titleId}>
      <h3 className="ui-setting-group-title" id={titleId}>{title}</h3>
      {children}
    </section>
  );
}
