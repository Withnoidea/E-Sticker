const assert = require("node:assert/strict");
const { test } = require("node:test");
const { listBreak } = require("../web/markdown.js");

const enter = (source, start = source.length, end = start) => {
  const change = listBreak(source, start, end);
  return change && source.slice(0, change.start) + change.text + source.slice(change.end);
};

test("unordered markers continue without losing their indentation", () => {
  for (const bullet of ["-", "*", "+"]) assert.equal(enter(`${bullet} first`), `${bullet} first\n${bullet} `);
  assert.equal(enter("- parent\n  - child"), "- parent\n  - child\n  - ");
  assert.equal(enter("\t* child"), "\t* child\n\t* ");
});
test("ordered lists increment and preserve the delimiter", () => {
  assert.equal(enter("9. first"), "9. first\n10. ");
  assert.equal(enter("10) first"), "10) first\n11) ");
});
test("new task list items always start unchecked", () => {
  for (const state of [" ", "x", "X"]) assert.equal(enter(`- [${state}] task`), `- [${state}] task\n- [ ] `);
  assert.equal(enter("  * [x] task"), "  * [x] task\n  * [ ] ");
});
test("enter on an empty item exits the list", () => {
  for (const marker of ["- ", "* ", "12. ", "- [ ] ", "  - "]) assert.equal(enter("- first\n" + marker), "- first\n");
});
test("quoted lists preserve the quote and exit to a quote", () => {
  assert.equal(enter("> - first"), "> - first\n> - ");
  assert.equal(enter("> - "), "> ");
});
test("splitting a list item preserves the trailing text and replaces selections", () => {
  assert.equal(enter("- first second", 8), "- first \n- second");
  assert.equal(enter("- first selected tail", 8, 17), "- first \n- tail");
});
test("plain paragraphs, partial markers, and fenced code are left to the editor", () => {
  for (const source of ["plain", "-", "---", "```md\n- code", "~~~\n1. code", "- example\n  ```\n  - code"]) assert.equal(enter(source), null);
  assert.equal(enter("```\ncode\n```\n- first"), "```\ncode\n```\n- first\n- ");
  assert.equal(enter("- item", 1), null);
});
