import type { ReactNode } from 'react';
import { confirmAction } from '../components/ConfirmDialog';
import { runAction } from './actions';
import './ui.css';

interface CommonProps {
  onClick(): unknown;
  children: ReactNode;
  disabled?: boolean;
  busy?: boolean;
}

export type ButtonProps =
  | (CommonProps & { kind: 'primary' | 'secondary' | 'quiet'; type?: 'button' | 'submit' })
  | (CommonProps & { kind: 'danger'; confirm: { message: string; label?: string } });

export default function Button(p: ButtonProps) {
  const type = p.kind === 'danger' ? 'button' : (p.type ?? 'button');
  return (
    <button
      type={type}
      className={`ui-button ui-button-${p.kind}`}
      disabled={p.disabled || p.busy}
      aria-busy={p.busy || undefined}
      onClick={() => { void runAction(p.kind, p.onClick, p.kind === 'danger' ? p.confirm : undefined, confirmAction); }}
    >
      {p.children}
    </button>
  );
}
