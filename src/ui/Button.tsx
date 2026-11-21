import { useRef, type ReactNode } from 'react';
import { confirmAction } from '../components/ConfirmDialog';
import { runAction } from './actions';
import './ui.css';

interface CommonProps {
  onClick(): unknown;
  children: ReactNode;
  disabled?: boolean;
  busy?: boolean;
  /** Called when onClick rejects. Without it the error is logged. */
  onError?(e: unknown): void;
}

export type ButtonProps =
  | (CommonProps & { kind: 'primary' | 'secondary' | 'quiet'; type?: 'button' | 'submit' })
  | (CommonProps & { kind: 'danger'; confirm: { message: string; label?: string } });

export default function Button(p: ButtonProps) {
  const type = p.kind === 'danger' ? 'button' : (p.type ?? 'button');
  const running = useRef(false);

  async function click() {
    if (running.current) return;
    running.current = true;
    try {
      await runAction(p.kind, p.onClick, p.kind === 'danger' ? p.confirm : undefined, confirmAction);
    } catch (e) {
      if (p.onError) p.onError(e);
      else console.error(e);
    } finally {
      running.current = false;
    }
  }
  return (
    <button
      type={type}
      className={`ui-button ui-button-${p.kind}`}
      disabled={p.disabled || p.busy}
      aria-busy={p.busy || undefined}
      onClick={() => { void click(); }}
    >
      {p.children}
    </button>
  );
}
