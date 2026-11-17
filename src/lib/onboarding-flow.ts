export type StartupScreen = 'workspace' | 'welcome' | 'setup' | 'quick';

/**
 * Whether the language in use should be saved for someone who never saved
 * one: anyone past the welcome screen, whose interface language would
 * otherwise differ from the backend's (an install from before the language
 * step, for one). The welcome screen asks first.
 */
export function keepsLocaleInUse(screen: StartupScreen | null): boolean {
  return screen === 'workspace' || screen === 'setup' || screen === 'quick';
}

export async function readStartupScreen(
  readComplete: () => Promise<boolean>,
  readIntro: () => Promise<string>,
): Promise<StartupScreen> {
  if (await readComplete()) return 'workspace';
  // Past the welcome but not finished: resume the quick setup. Mail setup is
  // one click from there and from Set up in the workspace.
  return await readIntro() === 'true' ? 'quick' : 'welcome';
}
