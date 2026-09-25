import test from 'node:test';
import assert from 'node:assert/strict';
import { initialWorkspace, workspaceReducer, restoreWorkspace, shouldFocusResult, pruneWorkspacePages, type WorkspacePages } from '../src/workspace/model.ts';

test('returning to a tool reuses its tab instead of losing its context', () => {
  let s = workspaceReducer(initialWorkspace(), { type: 'open', view: 'email' });
  const mail = s.activeId;
  s = workspaceReducer(s, { type: 'open', view: 'calendar' });
  s = workspaceReducer(s, { type: 'open', view: 'email' });
  assert.equal(s.activeId, mail);
  assert.equal(s.tabs.filter(t => t.view === 'email').length, 1);
});

test('closing a tab preserves its identity in history and reopening restores it', () => {
  let s = workspaceReducer(initialWorkspace(), { type: 'open', view: 'chat', id: 'task:a', sessionId: 'a', title: 'Follow-up' });
  s = workspaceReducer(s, { type: 'open', view: 'email' });
  s = workspaceReducer(s, { type: 'activate', id: 'task:a' });
  s = workspaceReducer(s, { type: 'close', id: 'task:a' });
  assert.equal(s.activeId, 'tool:email');
  assert.equal(s.history[0].sessionId, 'a');
  s = workspaceReducer(s, { type: 'open', view: 'chat', id: 'task:a', sessionId: 'a' });
  assert.equal(s.tabs.find(t => t.id === 'task:a')?.title, 'Follow-up');
});

test('starting another task never clears or reuses an existing task', () => {
  let s = workspaceReducer(initialWorkspace(), { type: 'open', view: 'chat', id: 'task:a', sessionId: 'a' });
  s = workspaceReducer(s, { type: 'open', view: 'chat', id: 'task:b', sessionId: 'b' });
  assert.deepEqual(s.tabs.filter(t => t.view === 'chat').map(t => t.sessionId), ['a', 'b']);
});

test('automation overview survives closing other tabs and cannot be closed', () => {
  let s = workspaceReducer(initialWorkspace(), { type: 'open', view: 'email' });
  s = workspaceReducer(s, { type: 'close', id: 'tool:email' });
  s = workspaceReducer(s, { type: 'close', id: 'automation' });
  assert.equal(s.activeId, 'automation');
  assert.equal(s.tabs.length, 1);
});

test('restoration rejects invalid destinations and deduplicates IDs', () => {
  const s = restoreWorkspace(JSON.stringify({activeId:'broken',tabs:[{id:'a',view:'email',title:'Mail'}, {id:'a',view:'calendar',title:'Duplicate'}, {id:'b',view:'javascript:alert(1)',title:'Bad'}],history:[]}));
  assert.equal(s.tabs.filter(t => t.id === 'a').length, 1);
  assert.ok(s.tabs.every(t => ['email','home'].includes(t.view)));
  assert.equal(s.activeId, 'automation');
  assert.deepEqual(restoreWorkspace('{bad'), initialWorkspace());
});

test('background results do not take focus from another workspace', () => {
  assert.equal(shouldFocusResult('task:a', 'task:a', 'chat'), true);
  assert.equal(shouldFocusResult('tool:email', 'task:a', 'email'), false);
  assert.equal(shouldFocusResult('automation', 'automation', 'home'), false);
});

test('the twenty-first closed result loses its cached body while reachable drafts survive', () => {
  let state = workspaceReducer(initialWorkspace(), { type: 'open', view: 'chat', id: 'task:active', sessionId: 'active' });
  const pages: WorkspacePages = { 'task:active': { draft: 'Unsent active draft' } };
  for (let index = 0; index < 21; index++) {
    const id = `result:${index}`;
    pages[id] = { toolResult: { body: `Original message ${index}` }, draft: `Draft ${index}` };
    state = workspaceReducer(state, { type: 'open', view: 'result', id });
    state = workspaceReducer(state, { type: 'close', id });
  }
  const retained = pruneWorkspacePages(state, pages);
  assert.equal(state.history.length, 20);
  assert.equal(retained['result:0'], undefined);
  assert.equal(JSON.stringify(retained).includes('Original message 0'), false);
  assert.equal(retained['task:active'].draft, 'Unsent active draft');
  assert.equal(retained['result:1'].draft, 'Draft 1');
  state = workspaceReducer(state, { type: 'open', view: 'result', id: 'result:1' });
  assert.equal(pruneWorkspacePages(state, retained)['result:1'].draft, 'Draft 1');
});

test('restoring pages discards unreachable data after layout validation', () => {
  const state = restoreWorkspace(JSON.stringify({
    tabs: [{ id: 'current', view: 'email', title: 'Mail' }],
    history: [{ id: 'task:recent', view: 'chat', title: 'Recent', sessionId: 'recent' }],
  }));
  const pages = {
    current: { query: 'Saved search' },
    'task:recent': { draft: 'Recoverable draft' },
    orphan: { toolResult: { body: 'Old inaccessible message' } },
  };
  assert.deepEqual(pruneWorkspacePages(state, pages), {
    current: pages.current,
    'task:recent': pages['task:recent'],
  });
  assert.deepEqual(pruneWorkspacePages(restoreWorkspace('{invalid'), pages), {});
});

test('a result written before opening is retained when the latest layout is saved', () => {
  const state = initialWorkspace();
  const pages = { 'result:new': { toolResult: { body: 'New result' } } };
  const latest = workspaceReducer(state, { type: 'open', view: 'result', id: 'result:new' });
  assert.deepEqual(pruneWorkspacePages(latest, pages), pages);
  assert.equal(pages['result:new'].toolResult.body, 'New result');
});

test('a dock-only assistant keeps its unsent draft without any conversation tab', () => {
  const state = initialWorkspace();
  const pages: WorkspacePages = {
    'task:legacy': { assistantDraft: 'Unsent dock draft' },
    'task:empty': { assistantDraft: '' },
    'result:old': { toolResult: { body: 'Old message' } },
  };
  assert.deepEqual(pruneWorkspacePages(state, pages), {
    'task:legacy': pages['task:legacy'],
  });
});

test('the dock draft survives after its conversation leaves the twenty-tab history', () => {
  let state = workspaceReducer(initialWorkspace(), { type: 'open', view: 'chat', id: 'task:dock', sessionId: 'dock' });
  state = workspaceReducer(state, { type: 'close', id: 'task:dock' });
  for (let index = 0; index < 20; index++) {
    const id = `result:${index}`;
    state = workspaceReducer(state, { type: 'open', view: 'result', id });
    state = workspaceReducer(state, { type: 'close', id });
  }
  assert.equal(state.history.some(tab => tab.id === 'task:dock'), false);
  const pages = { 'task:dock': { assistantDraft: 'Continue this unsent draft' } };
  assert.deepEqual(pruneWorkspacePages(state, pages), pages);
  assert.deepEqual(pruneWorkspacePages(restoreWorkspace(JSON.stringify(state)), pages), pages);
});
