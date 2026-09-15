import assert from "node:assert/strict";
import { test } from "node:test";
import { tokenize } from "../../src/spawn/tokenizer.ts";

test("tokenize keeps quoted spaces as one argv", () => {
  assert.deepEqual(tokenize('/bin/echo "hello world"'), {
    kind: "ok",
    argv: ["/bin/echo", "hello world"],
  });
});

test("tokenize fails on unmatched quotes", () => {
  assert.deepEqual(tokenize('/bin/echo "hello'), { kind: "fail" });
});
