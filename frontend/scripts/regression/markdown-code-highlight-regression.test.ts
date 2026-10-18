import test from 'node:test';
import assert from 'node:assert/strict';

import { renderMarkdown } from '../../src/utils/markdownCore';

const fenced = (lang: string, body: string): string =>
  renderMarkdown(['```' + lang, body, '```'].join('\n'));

test('slint fences highlight comments, strings, keywords and dimensions', () => {
  const html = fenced('slint', [
    '// 主窗口',
    'export component MainWindow inherits Window {',
    '  in-out property <int> count: 0;',
    '  Text {',
    '    text: "hello";',
    '    font-size: 14px;',
    '  }',
    '}'
  ].join('\n'));
  assert.match(html, /<span class="token-comment">\/\/ 主窗口<\/span>/);
  assert.match(html, /<span class="token-keyword">export<\/span>/);
  assert.match(html, /<span class="token-keyword">component<\/span>/);
  assert.match(html, /<span class="token-string">&quot;hello&quot;<\/span>/);
  assert.match(html, /<span class="token-number">14px<\/span>/);
  assert.match(html, /<span class="token-number">0<\/span>/);
  assert.match(html, /<span class="token-keyword">&lt;int&gt;<\/span>/);
});

test('slint keeps the compound property direction intact', () => {
  const html = fenced('slint', 'in-out property <string> label;');
  // A shorter `in` branch listed first would split this into `in` + `-out`.
  assert.match(html, /<span class="token-keyword">in-out<\/span>/);
  assert.doesNotMatch(html, /token-keyword">in<\/span><\/span>-out|token-keyword">in<\/span>-out/);
});

test('slint colour literals are data, not comments', () => {
  const html = fenced('slint', 'background: #f8fafc;');
  assert.match(html, /<span class="token-number">#f8fafc<\/span>/);
  assert.doesNotMatch(html, /token-comment">#f8fafc/);
});

test('script numbers fall into the number bucket', () => {
  const html = fenced('ts', 'const total = 42;');
  assert.match(html, /<span class="token-number">42<\/span>/);
});

test('rust highlights keywords, strings and suffixed numbers', () => {
  const html = fenced('rust', [
    '// entry point',
    'fn main() {',
    '    let total: u32 = 42;',
    '    println!("done");',
    '}'
  ].join('\n'));
  assert.match(html, /<span class="token-comment">\/\/ entry point<\/span>/);
  assert.match(html, /<span class="token-keyword">fn<\/span>/);
  assert.match(html, /<span class="token-keyword">let<\/span>/);
  assert.match(html, /<span class="token-keyword">u32<\/span>/);
  assert.match(html, /<span class="token-number">42<\/span>/);
  assert.match(html, /<span class="token-string">&quot;done&quot;<\/span>/);
});

test('go accepts its common alias and highlights func', () => {
  assert.match(fenced('go', 'func main() {}'), /<span class="token-keyword">func<\/span>/);
  assert.match(fenced('golang', 'package main'), /<span class="token-keyword">package<\/span>/);
});

test('c preprocessor directives are keywords, not comments', () => {
  const html = fenced('c', ['#include <stdio.h>', '// ready', 'int main() { return 0; }'].join('\n'));
  assert.match(html, /<span class="token-keyword">#include<\/span>/);
  assert.match(html, /<span class="token-comment">\/\/ ready<\/span>/);
  assert.match(html, /<span class="token-keyword">return<\/span>/);
  assert.match(html, /<span class="token-number">0<\/span>/);
});

test('yaml keys and toml sections highlight without keywords', () => {
  assert.match(fenced('yaml', 'threads: 4'), /<span class="token-keyword">threads<\/span>/);
  assert.match(fenced('toml', '[package]\nname = "wunder"'), /<span class="token-keyword">\[package\]<\/span>/);
});

test('css colours stay data while c-style comments stay comments', () => {
  const html = fenced('css', ['/* theme */', '.box { color: #f8fafc; }'].join('\n'));
  assert.match(html, /<span class="token-comment">\/\* theme \*\/<\/span>/);
  assert.match(html, /<span class="token-keyword">color<\/span>/);
  assert.match(html, /<span class="token-number">#f8fafc<\/span>/);
});

test('markup languages highlight tags', () => {
  const html = fenced('html', '<div class="row">text</div>');
  assert.match(html, /<span class="token-keyword">&lt;div<\/span>/);
  assert.match(html, /<span class="token-string">&quot;row&quot;<\/span>/);
});

test('sql stays case insensitive after the lexer rewrite', () => {
  assert.match(fenced('sql', 'SELECT * FROM t'), /<span class="token-keyword">SELECT<\/span>/);
  assert.match(fenced('sql', 'select * from t'), /<span class="token-keyword">select<\/span>/);
});

test('every registered language tokenises its comment form', () => {
  const commentMarkers: Record<string, string> = {
    rust: '//', go: '//', java: '//', kotlin: '//', swift: '//', c: '//', cpp: '//',
    csharp: '//', php: '//', ruby: '#', scala: '//', lua: '--', dart: '//',
    yaml: '#', toml: '#', ini: ';', xml: '<!--', css: '/*'
  };
  for (const [language, marker] of Object.entries(commentMarkers)) {
    const source = language === 'xml' ? '<!-- note -->' : language === 'css' ? '/* note */' : `${marker} note`;
    const html = fenced(language, source);
    assert.match(html, /token-comment/, `${language} comment`);
  }
});

test('unregistered languages stay plain', () => {
  const html = fenced('zig', 'const total: u32 = 42;');
  assert.doesNotMatch(html, /token-/);
  assert.match(html, /<code class="language-zig">const total: u32 = 42;/);
});

test('ini accepts a semicolon comment without staining other languages', () => {
  assert.match(fenced('ini', '; note'), /<span class="token-comment">; note<\/span>/);
  // A semicolon is a statement terminator here, never a comment.
  assert.doesNotMatch(fenced('java', 'int x = 1;'), /token-comment/);
});
