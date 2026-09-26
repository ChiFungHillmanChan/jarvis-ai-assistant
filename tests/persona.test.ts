import test from 'node:test';
import assert from 'node:assert/strict';
import { PERSONA_CHOICES, personaLabel, personaValue } from '../src/lib/persona.ts';

const en = (value: string) => value;
const zh = (_: string, value: string) => value;

test('an unsaved or unknown choice shows the default the assistant falls back to', () => {
  assert.equal(personaValue('assistant_tone', null), 'jarvis');
  assert.equal(personaValue('assistant_tone', 'sarcastic'), 'jarvis');
  assert.equal(personaValue('assistant_length', 'brief'), 'brief');
  assert.equal(personaValue('assistant_chinese', null), 'written');
  assert.equal(personaValue('assistant_search', ''), 'thorough');
});

test('every choice has an English and a Chinese label', () => {
  for (const values of Object.values(PERSONA_CHOICES)) {
    for (const value of values) {
      assert.notEqual(personaLabel(value, en), value, `${value} has no English label`);
      assert.match(personaLabel(value, zh), /[一-鿿]/, `${value} has no Chinese label`);
    }
  }
});
