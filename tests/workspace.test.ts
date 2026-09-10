import test from 'node:test';
import assert from 'node:assert/strict';
import { initialWorkspace, workspaceReducer, restoreWorkspace, shouldFocusResult } from '../src/workspace/model.ts';

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
