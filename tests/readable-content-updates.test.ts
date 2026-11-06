import test from 'node:test';
import assert from 'node:assert/strict';
import { byTag, document } from './dom.ts';
import { createElement } from 'react';
import { flushSync } from 'react-dom';
import { createRoot } from 'react-dom/client';
import { ContentBody } from '../src/components/content/content-body.ts';

// A streamed reply re-renders its bubble on every frame, and its parent passes
// new callbacks each time, as ReadableContent did.
function mount() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container as unknown as HTMLElement);
  const render = (text: string) => flushSync(() => root.render(createElement(ContentBody, { text, tableLabel: 'Table', onOpenLink: () => {}, quoteLabels: { show: () => 'Show', hide: 'Hide' } })));
  return { container, render };
}

test('a streamed table keeps its table element while new text arrives', () => {
  const { container, render } = mount();
  render('| File | Due |\n| --- | --- |\n| Report | 30 Sept |\n| Invoice | 2 Oct |\n\nMore');
  const [table] = byTag(container, 'table');
  render('| File | Due |\n| --- | --- |\n| Report | 30 Sept |\n| Invoice | 2 Oct |\n\nMore text');
  assert.ok(table, 'the table renders');
  // Compare identity with ok(): a failing equal() would diff two whole DOM trees.
  assert.ok(byTag(container, 'table')[0] === table, 'the same table element is still on screen');
});

test('a streamed link keeps its anchor element while new text arrives', () => {
  const { container, render } = mount();
  render('See [the report](https://example.com/report) for');
  const [link] = byTag(container, 'a');
  render('See [the report](https://example.com/report) for details');
  assert.ok(link, 'the link renders');
  assert.ok(byTag(container, 'a')[0] === link, 'the same anchor element is still on screen');
});

test('re-rendering the same text creates no DOM nodes', () => {
  const { container, render } = mount();
  const text = 'Intro\n\n| A | B |\n| --- | --- |\n| 1 | [two](https://example.com/2) |';
  render(text);
  let created = 0;
  const createElementBefore = document.createElement, createTextNodeBefore = document.createTextNode;
  document.createElement = (tag: string) => { created++; return createElementBefore.call(document, tag); };
  document.createTextNode = (value: string) => { created++; return createTextNodeBefore.call(document, value); };
  try { render(text); } finally { document.createElement = createElementBefore; document.createTextNode = createTextNodeBefore; }
  assert.equal(created, 0);
  assert.equal(byTag(container, 'table').length, 1);
});
