import test from 'node:test';
import assert from 'node:assert/strict';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { ContentBody, type ContentBodyProps } from '../src/components/content/content-body.ts';

const render = (text: string, props: Omit<ContentBodyProps, 'text'> = {}) =>
  renderToStaticMarkup(createElement(ContentBody, { text, ...props }));

test('AI mail results render tables, lists and emphasis as a document', () => {
  const html = render('## 待辦事項\n\n- **提交報告**\n\n| 文件 | 到期日 |\n| --- | --- |\n| 報告 | 9 月 30 日 |\n\n> 以原信為準');
  assert.match(html, /<h2>待辦事項<\/h2>/);
  assert.match(html, /<li><strong>提交報告<\/strong><\/li>/);
  assert.match(html, /<table>/);
  assert.match(html, /<th>文件<\/th>/);
  assert.match(html, /<blockquote>/);
  assert.doesNotMatch(html, /\*\*|## |\| ---/);
});

test('a whole response wrapped as markdown renders normally while code stays literal', () => {
  const html = render('```markdown\n# 文件摘要\n\n```js\nconst value = "**literal**";\n```\n```');
  assert.match(html, /<h1>文件摘要<\/h1>/);
  assert.match(html, /<pre><code class="language-js">const value = &quot;\*\*literal\*\*&quot;;\n<\/code><\/pre>/);
  assert.doesNotMatch(html, /```/);
});

test('only validated mail sources become inline controls and code remains literal', () => {
  const html = render('期限 [郵件 #42]；未知 [郵件 #99]。\n\n`[郵件 #42]`', { sourceIds: [42], onSource: () => {} });
  assert.match(html, /<button[^>]+data-source-id="42"[^>]*>郵件 #42<\/button>/);
  assert.match(html, /未知 \[郵件 #99\]/);
  assert.match(html, /<code>\[郵件 #42\]<\/code>/);
  assert.equal((html.match(/<button/g) || []).length, 1);
});

test('raw email keeps source symbols and line breaks unchanged', () => {
  const source = '## Subject\n**literal**\n[CHART:bar|{"value":2}]\n<sender@example.test>\nA_B_C';
  const html = render(source, { format: 'plain' });
  assert.match(html, /## Subject\n\*\*literal\*\*/);
  assert.match(html, /&lt;sender@example.test&gt;/);
  assert.match(html, /A_B_C/);
  assert.doesNotMatch(html, /<h2>|<strong>|<a |<svg/);
});

test('untrusted Markdown cannot render HTML, unsafe links or tracking images', () => {
  const html = render('<script>alert(1)</script>\n\n[bad](javascript:alert%281%29) [file](file:///etc/passwd) [good](https://example.test/report)\n\n![Report scan](https://example.test/pixel?private=1)');
  assert.doesNotMatch(html, /<script|<img|href="javascript:|href="file:|src=/);
  assert.match(html, /Report scan/);
  assert.match(html, /href="https:\/\/example.test\/report"/);
  assert.match(html, /rel="noopener noreferrer"/);
});

test('relative and credential-bearing links stay readable without navigating the app', () => {
  const html = render('[local](/settings) [secret](https://password@example.test/) [email](mailto:help@example.test)');
  assert.doesNotMatch(html, /href="\/settings|href="https:\/\/password/);
  assert.match(html, /local/);
  assert.match(html, /href="mailto:help@example.test"/);
});
