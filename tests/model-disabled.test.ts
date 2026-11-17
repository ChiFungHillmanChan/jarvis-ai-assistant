import test from 'node:test';
import assert from 'node:assert/strict';
import { choiceKey, noticesToShow, noticeText, placeOf, type DisabledChoice } from '../src/lib/model-disabled.ts';

const chain = (model: string, replacement: string | null): DisabledChoice => ({ place: 'chain', position: 0, provider: 'openai', model, replacement, replacement_label: replacement ? 'Luna' : null });
const mail: DisabledChoice = { place: 'mail', field: 'openai_model', provider: 'openai', model: 'old-model', replacement: 'new-model', replacement_label: null };
const en = (english: string) => english;
const zh = (_: string, chinese: string) => chinese;

test('a disabled model is shown once on its own, and again only when a request on it stops', () => {
  const choice = chain('old-model', 'new-model');
  assert.deepEqual(noticesToShow([], [choice], new Set(), null), [choice], 'first time');
  const seen = new Set([choiceKey(choice)]);
  assert.deepEqual(noticesToShow([], [choice], seen, null), [], 'seen: not on its own');
  assert.deepEqual(noticesToShow([], [choice], seen, { provider: 'openai', model: 'old-model', replacement: 'new-model' }), [choice], 'a stopped request opens it again');
  assert.deepEqual(noticesToShow([], [choice], seen, { provider: 'openai', model: 'other', replacement: null }), [], 'another model stopping does not');
  assert.deepEqual(noticesToShow([choice], [choice], seen, null), [choice], 'an open notice stays open');
  assert.deepEqual(noticesToShow([choice], [], new Set(), null), [], 'a replaced choice drops out');
});

test('a new replacement for the same saved model is a new notice', () => {
  assert.notEqual(choiceKey(chain('old-model', 'a')), choiceKey(chain('old-model', 'b')));
  assert.notEqual(choiceKey(chain('old-model', 'a')), choiceKey({ ...mail, model: 'old-model', replacement: 'a' }));
});

test('confirm sends the place as disabled.rs reads it', () => {
  assert.deepEqual(placeOf(chain('m', 'r')), { place: 'chain', position: 0 });
  assert.deepEqual(placeOf(mail), { place: 'mail', field: 'openai_model' });
});

test('the notice names the model, the replacement, and that nothing switches on its own', () => {
  const withReplacement = noticeText(chain('old-model', 'new-model'), zh);
  assert.match(withReplacement, /old-model/);
  assert.match(withReplacement, /Luna（new-model）/);
  assert.match(withReplacement, /不會自行更換模型/);
  assert.match(noticeText(mail, en), /Suggested replacement: new-model\./);
  const none = noticeText(chain('old-model', null), en);
  assert.doesNotMatch(none, /Suggested/);
  assert.match(none, /Choose another model/);
});
