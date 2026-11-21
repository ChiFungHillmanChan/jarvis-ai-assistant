export type ActionKind = 'primary' | 'secondary' | 'quiet' | 'danger';

/**
 * Runs a button's action. Only a danger action confirms: it must carry a confirm
 * message and runs only if `ask` resolves true. Other kinds run straight away and
 * ignore `confirm`. Returns whether the action ran.
 */
export async function runAction(
  kind: ActionKind,
  action: () => unknown,
  confirm: { message: string; label?: string } | undefined,
  ask: (message: string, label?: string) => Promise<boolean>,
): Promise<boolean> {
  if (kind === 'danger') {
    if (!confirm) throw new Error('danger actions need a confirm message');
    if (!(await ask(confirm.message, confirm.label))) return false;
  }
  await action();
  return true;
}
