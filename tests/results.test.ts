import test from 'node:test';
import assert from 'node:assert/strict';
import { validResult, resultIdentity, providerUrl, type ToolResult } from '../src/workspace/results.ts';
import { matchesRun, mergeHistory } from '../src/workspace/chatState.ts';
const email: ToolResult = { session_id: 'a', run_id: 'run', tool_call_id: 'call', tool_name: 'read_email', target: { view: 'email', store: 'legacy', accountId: 'legacy-google', messageId: 'abc' }, result: { content: 'Example email' } };
test('legacy mail results cannot route into native Mail numeric IDs', () => {
  assert.equal(validResult(email), true);
  assert.equal(validResult({ ...email, target: { view:'email', store:'native', accountId:1, messageId:1 } }), false);
  assert.equal(validResult({ ...email, target: { view:'https://example.com' } }), false);
  assert.equal(validResult({ ...email, tool_call_id:null }), false);
});
test('repeated provider objects share identity, different objects stay separate', () => {
  assert.equal(resultIdentity(email), resultIdentity({ ...email, session_id:'b', run_id:'new', tool_call_id:'other' }));
  assert.notEqual(resultIdentity(email), resultIdentity({ ...email, target: { ...email.target, messageId:'def' } }));
});
test('provider links reject unexpected hosts and credentials', () => {
  const github: ToolResult = { ...email, tool_name:'create_github_issue', target:{view:'github',repo:'a/b'}, result:{providerUrl:'https://github.com/a/b/issues/1'} };
  assert.equal(providerUrl(github), 'https://github.com/a/b/issues/1');
  assert.equal(providerUrl({...github,result:{providerUrl:'https://github.com.evil.test/a/b'}}), null);
  assert.equal(providerUrl({...github,result:{providerUrl:'https://secret@github.com/a/b'}}), null);
});
test('tokens/status/results require both originating task and current run', () => {
  const current = {sessionId:'a',runId:'r2'};
  assert.equal(matchesRun({session_id:'a',run_id:'r2'}, current), true);
  assert.equal(matchesRun({session_id:'b',run_id:'r2'}, current), false);
  assert.equal(matchesRun({session_id:'a',run_id:'r1'}, current), false);
  assert.equal(matchesRun({}, current), false);
  assert.equal(matchesRun({session_id:'a',run_id:'r2'}, null), false);
});
test('history hydration retains prior turns and concurrent voice events exactly once', () => {
  const old = {id:1,role:'user' as const,content:'Earlier task',created_at:null};
  const pending = {id:null,role:'assistant' as const,content:'Voice answer',created_at:null};
  assert.deepEqual(mergeHistory([old], [pending]), [old,pending]);
  assert.deepEqual(mergeHistory([old,pending], [pending]), [old,pending]);
});
