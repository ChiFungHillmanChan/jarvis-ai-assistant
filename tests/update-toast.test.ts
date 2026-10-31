import test from 'node:test';
import assert from 'node:assert/strict';
import {
  RECHECK_MS,
  RESTART_GRACE_MS,
  RETRY_MS,
  createUpdateRunner,
  isToastVisible,
  type PendingUpdate,
} from '../src/lib/update.ts';

// The toast is the only update affordance in the app, so the rules that decide
// when an update downloads, when the toast is on screen and what Restart does
// are the rules that decide whether a user can update at all. They live here
// rather than inside the component.

const settle = () => new Promise(resolve => setImmediate(resolve));

function harness(options: {
  check: () => Promise<PendingUpdate | null>;
  relaunch?: () => Promise<void>;
}) {
  const timers: { run: () => void; delay: number }[] = [];
  const runner = createUpdateRunner({
    check: options.check,
    relaunch: options.relaunch ?? (async () => {}),
    schedule: (run, delay) => {
      const timer = { run, delay };
      timers.push(timer);
      return () => { timers.splice(timers.indexOf(timer), 1); };
    },
  });
  /** Fires the one pending timer, asserting it was scheduled for `delay`. */
  async function fire(delay: number) {
    assert.equal(timers.length, 1, 'exactly one timer is pending');
    assert.equal(timers[0].delay, delay);
    timers.shift()!.run();
    await settle();
  }
  return { runner, timers, fire };
}

function update(version: string, overrides: Partial<PendingUpdate> = {}) {
  const calls: string[] = [];
  const pending: PendingUpdate = {
    version,
    download: async () => { calls.push('download'); },
    install: async () => { calls.push('install'); },
    close: async () => { calls.push('close'); },
    ...overrides,
  };
  return { pending, calls };
}

test('nothing is shown when there is no update, and the next check is hours away', async () => {
  const { runner, fire } = harness({ check: async () => null });
  runner.start();
  await settle();
  assert.equal(isToastVisible(runner.getState()), false);
  await fire(RECHECK_MS);
});

test('an update downloads in the background and the toast appears only once it is ready', async () => {
  let finishDownload!: () => void;
  const { pending, calls } = update('0.5.7', {
    download: () => new Promise<void>(resolve => { finishDownload = resolve; }),
  });
  const { runner, timers } = harness({ check: async () => pending });
  runner.start();
  await settle();
  // Downloading is silent: no click, and nothing on screen to wait on.
  assert.equal(isToastVisible(runner.getState()), false);
  finishDownload();
  await settle();
  const state = runner.getState();
  assert.equal(isToastVisible(state), true);
  assert.equal(state.phase, 'ready');
  assert.equal(state.version, '0.5.7');
  assert.deepEqual(calls, [], 'nothing is installed until the user restarts');
  // A downloaded update is kept until restart; checking again would only
  // download the same version a second time.
  assert.equal(timers.length, 0);
});

test('a download that fails or stalls retries in minutes, without a toast', async () => {
  // A stalled connection never errors on its own. The download timeout turns
  // it into this failure; before, the toast sat at the same percentage forever
  // and the user could neither retry nor dismiss it.
  const failing = update('0.5.7', { download: async () => { throw new Error('operation timed out'); } });
  const working = update('0.5.7');
  const results = [failing.pending, working.pending];
  const { runner, fire } = harness({ check: async () => results.shift()! });
  runner.start();
  await settle();
  assert.equal(isToastVisible(runner.getState()), false);
  assert.deepEqual(failing.calls, ['close'], 'the failed handle is released');
  await fire(RETRY_MS);
  assert.equal(runner.getState().phase, 'ready');
});

test('a check that fails retries in minutes', async () => {
  const { pending } = update('0.5.7');
  const results: (() => Promise<PendingUpdate | null>)[] = [
    async () => { throw new Error('offline'); },
    async () => pending,
  ];
  const { runner, fire } = harness({ check: () => results.shift()!() });
  runner.start();
  await settle();
  await fire(RETRY_MS);
  assert.equal(runner.getState().phase, 'ready');
});

test('start runs once per session however many toasts mount', async () => {
  // PaywallGate and the workspace each render the toast. Two runners would
  // download twice and could install over each other.
  let checks = 0;
  const { runner } = harness({ check: async () => { checks += 1; return null; } });
  runner.start();
  runner.start();
  await settle();
  assert.equal(checks, 1);
});

test('restart installs the downloaded update, then relaunches', async () => {
  const order: string[] = [];
  const { pending } = update('0.5.7', { install: async () => { order.push('install'); } });
  const { runner } = harness({ check: async () => pending, relaunch: async () => { order.push('relaunch'); } });
  runner.start();
  await settle();
  void runner.restart();
  assert.equal(runner.getState().phase, 'restarting');
  await settle();
  assert.deepEqual(order, ['install', 'relaunch']);
});

test('a restarting toast cannot be dismissed and ignores a second click', async () => {
  let installs = 0;
  const { pending } = update('0.5.7', { install: () => { installs += 1; return new Promise<void>(() => {}); } });
  const { runner } = harness({ check: async () => pending });
  runner.start();
  await settle();
  void runner.restart();
  void runner.restart();
  runner.dismiss();
  await settle();
  assert.equal(installs, 1);
  assert.equal(isToastVisible(runner.getState()), true);
});

test('a failed install stays on screen with the reason, and Restart tries again', async () => {
  const attempts: (() => Promise<void>)[] = [
    async () => { throw new Error('Failed to move the new app into place'); },
    async () => {},
  ];
  let relaunched = false;
  const { pending } = update('0.5.7', { install: () => attempts.shift()!() });
  const { runner } = harness({ check: async () => pending, relaunch: async () => { relaunched = true; } });
  runner.start();
  await settle();
  await runner.restart();
  let state = runner.getState();
  assert.equal(state.phase, 'install-failed');
  assert.equal(state.message, 'Error: Failed to move the new app into place');
  assert.equal(relaunched, false);
  await runner.restart();
  state = runner.getState();
  assert.equal(state.phase, 'restarting');
  assert.equal(state.message, null);
  assert.equal(relaunched, true);
});

test('when relaunch fails, the toast asks the user to reopen JARVIS', async () => {
  // The new version is already installed at this point, so quitting and
  // reopening finishes the update.
  const { pending } = update('0.5.7');
  const { runner } = harness({ check: async () => pending, relaunch: async () => { throw new Error('restart refused'); } });
  runner.start();
  await settle();
  await runner.restart();
  assert.equal(runner.getState().phase, 'restart-failed');
  assert.equal(isToastVisible(runner.getState()), true);
});

test('an app still running after relaunch asks the user to reopen JARVIS', async () => {
  const { pending } = update('0.5.7');
  const { runner, fire } = harness({ check: async () => pending });
  runner.start();
  await settle();
  await runner.restart();
  assert.equal(runner.getState().phase, 'restarting');
  await fire(RESTART_GRACE_MS);
  assert.equal(runner.getState().phase, 'restart-failed');
});

test('dismissing a ready update hides it for the rest of the session', async () => {
  const { pending } = update('0.5.7');
  const { runner, timers } = harness({ check: async () => pending });
  runner.start();
  await settle();
  runner.dismiss();
  assert.equal(isToastVisible(runner.getState()), false);
  assert.equal(timers.length, 0, 'no check brings it back; the next launch offers it again');
});

test('subscribers hear every change and can unsubscribe', async () => {
  const { pending } = update('0.5.7');
  const { runner } = harness({ check: async () => pending });
  const phases: string[] = [];
  const unsubscribe = runner.subscribe(() => phases.push(runner.getState().phase));
  runner.start();
  await settle();
  unsubscribe();
  runner.dismiss();
  assert.deepEqual(phases, ['ready']);
});
