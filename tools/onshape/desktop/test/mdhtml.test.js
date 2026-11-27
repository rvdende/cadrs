// Unit tests for the small Markdown -> HTML convertor used for PDF exports.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { toHtml } = require('../mdhtml');

test('toHtml: headings, paragraphs and title', () => {
  const html = toHtml('# Title\n\n## Section\n\nSome text.', 'Report');
  assert.match(html, /^<!doctype html><meta charset="utf-8"><title>Report<\/title>/);
  assert.match(html, /<h1>Title<\/h1>/);
  assert.match(html, /<h2>Section<\/h2>/);
  assert.match(html, /<p>Some text\.<\/p>/);
});

test('toHtml: tables, with the separator row consumed', () => {
  const html = toHtml('# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n');
  assert.match(html, /<table><thead><tr><th>a<\/th><th>b<\/th><\/tr><\/thead><tbody>/);
  assert.match(html, /<tr><td>1<\/td><td>2<\/td><\/tr><tr><td>3<\/td><td>4<\/td><\/tr>/);
  assert.doesNotMatch(html, /---/);
});

test('toHtml: lists', () => {
  const html = toHtml('- one\n- two\n\nAfter.');
  assert.match(html, /<ul><li>one<\/li><li>two<\/li><\/ul>/);
  assert.match(html, /<p>After\.<\/p>/);
});

test('toHtml: inline code, italics and HTML escaping', () => {
  const html = toHtml('A `code` and _emphasis_ and <script>alert(1)</script>');
  assert.match(html, /<code>code<\/code>/);
  assert.match(html, /<em>emphasis<\/em>/);
  assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/);
  assert.doesNotMatch(html, /<script>alert/);
});

test('toHtml: empty input gives a valid document', () => {
  const html = toHtml('');
  assert.match(html, /^<!doctype html>/);
  assert.match(html, /<\/style><\/head>|<\/style>/);
});
