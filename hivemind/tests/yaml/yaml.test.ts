import assert from "node:assert/strict";
import { test } from "node:test";
import { parseYaml } from "../../src/yaml/yaml.ts";

test("parseYaml returns an empty map for blank or comment-only text", () => {
  assert.deepEqual(parseYaml(""), {});
  assert.deepEqual(parseYaml("# only a comment\n\n"), {});
});

test("parseYaml parses scalars, nested maps, and lists", () => {
  const parsed = parseYaml(
    [
      "name: hive",
      "count: 2",
      "flag: true",
      "off: false",
      "empty: null",
      "tilde: ~",
      "nested:",
      "  child: 1",
      "items:",
      "  - a",
      "  - b: 2",
      "inline: [1, true]",
      "none: []",
      "obj: {}",
      'quoted: "hi # still text"',
      "",
    ].join("\n"),
  );
  assert.deepEqual(parsed, {
    name: "hive",
    count: 2,
    flag: true,
    off: false,
    empty: null,
    tilde: null,
    nested: { child: 1 },
    items: ["a", { b: 2 }],
    inline: [1, true],
    none: [],
    obj: {},
    quoted: "hi # still text",
  });
});

test("parseYaml unquotes strings and accepts CRLF lines", () => {
  assert.deepEqual(parseYaml('msg: "line\\nwith \\"q\\""\n'), {
    msg: 'line\nwith "q"',
  });
  assert.deepEqual(parseYaml("msg: 'it''s'\n"), { msg: "it's" });
  assert.deepEqual(parseYaml("a: 1\r\nb: 2\r\n"), { a: 1, b: 2 });
});

test("parseYaml omits a key with no value and no children", () => {
  assert.deepEqual(parseYaml("a:\nb: 1\n"), { b: 1 });
});

test("parseYaml rejects anchors, block scalars, and nested maps", () => {
  assert.throws(
    () => parseYaml("a: &foo 1\n"),
    /YAML anchors are not supported/,
  );
  assert.throws(() => parseYaml("a: *foo\n"), /YAML anchors are not supported/);
  assert.throws(
    () => parseYaml("a: |\n  body\n"),
    /Block scalars are not supported/,
  );
  assert.throws(
    () => parseYaml("a: {b: 1}\n"),
    /Nested maps are not supported/,
  );
});

test("parseYaml rejects indent, mixed, and unkeyed list errors", () => {
  assert.throws(() => parseYaml("  a: 1\n"), /Unexpected indent/);
  assert.throws(() => parseYaml("a: 1\n  b: 2\n"), /Mixed value and nested/);
  assert.throws(() => parseYaml("- a\n"), /List item without a key/);
  assert.throws(() => parseYaml("hello world\n"), /Cannot parse YAML/);
});
