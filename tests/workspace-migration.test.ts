import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dropRemovedViews, restoreWorkspace, views, type WorkspaceState, type WorkspaceView } from '../src/workspace/model.ts';

// Settings left the workspace for its own dialog (PR 6). A layout saved before
// that may still hold a settings tab; it must load without it.

const home = { id: 'automation', view: 'home' as WorkspaceView, title: 'Automation', pinned: true };
const settings = { id: 'tool:settings', view: 'settings' as WorkspaceView, title: 'Settings' };
const mail = { id: 'tool:email', view: 'email' as WorkspaceView, title: 'Mail' };

test('settings is no longer a workspace view', () => {
  assert.ok(!(views as readonly string[]).includes('settings'));
});

test('a saved active settings tab is dropped and home becomes active', () => {
  const saved: WorkspaceState = { tabs: [home, settings], activeId: 'tool:settings', history: [] };
  const next = dropRemovedViews(saved);
  assert.deepEqual(next.tabs, [home]);
  assert.equal(next.activeId, 'automation');
});

test('a state without a settings tab is returned unchanged', () => {
  const saved: WorkspaceState = { tabs: [home, mail], activeId: 'tool:email', history: [{ id: 'task:a', view: 'chat', title: 'Follow-up', sessionId: 'a' }], previousId: 'automation' };
  const copy = structuredClone(saved);
  assert.deepEqual(dropRemovedViews(saved), copy);
});

test('a settings tab that is not active is dropped and the active tab is kept', () => {
  const saved: WorkspaceState = { tabs: [home, settings, mail], activeId: 'tool:email', history: [settings], previousId: 'tool:settings' };
  const next = dropRemovedViews(saved);
  assert.deepEqual(next.tabs, [home, mail]);
  assert.equal(next.activeId, 'tool:email');
  assert.deepEqual(next.history, []);
  assert.equal(next.previousId, undefined);
});

test('a saved layout with a settings tab restores without it', () => {
  const restored = dropRemovedViews(restoreWorkspace(JSON.stringify({ tabs: [home, settings], activeId: 'tool:settings', history: [settings] })));
  assert.deepEqual(restored.tabs.map(t => t.view), ['home']);
  assert.equal(restored.activeId, 'automation');
  assert.deepEqual(restored.history, []);
});

test('the workspace applies the migration where it reads the saved layout', () => {
  const context = readFileSync(new URL('../src/workspace/WorkspaceContext.tsx', import.meta.url), 'utf8');
  assert.match(context, /const layout = dropRemovedViews\(restoreWorkspace\(settings\.values\.workspace_layout_v1\)\);/);
});
