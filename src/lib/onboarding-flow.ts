export type StartupScreen = 'workspace' | 'welcome' | 'setup';

export async function readStartupScreen(
  readComplete: () => Promise<boolean>,
  readIntro: () => Promise<string>,
): Promise<StartupScreen> {
  if (await readComplete()) return 'workspace';
  return await readIntro() === 'true' ? 'setup' : 'welcome';
}
