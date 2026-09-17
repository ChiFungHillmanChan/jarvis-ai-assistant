import test from 'node:test';
import assert from 'node:assert/strict';
import { startOperation, cancelOperation, isCurrentOperation, finishOperation, currentOperation } from '../src/lib/connection-operation.ts';

test('a service has one operation across mounted tabs and releases after completion', () => {
  const first = startOperation('github', 'connect')!;
  try {
    assert.equal(startOperation('github', 'disconnect'), null);
    assert.equal(currentOperation('github'), first);
    const other = startOperation('notion', 'connect')!;
    assert.ok(other);
    finishOperation(other);
  } finally { finishOperation(first); }
  assert.equal(currentOperation('github'), null);
});

test('cancelled startup cannot consume late success or cancel a newer operation', () => {
  const old = startOperation('github', 'connect')!;
  assert.equal(cancelOperation(old), true);
  assert.equal(isCurrentOperation(old), false);
  assert.equal(startOperation('github', 'connect'), null);
  finishOperation(old);
  const next = startOperation('github', 'connect')!;
  assert.equal(cancelOperation(old), false);
  finishOperation(old);
  assert.equal(isCurrentOperation(next), true);
  finishOperation(next);
});
