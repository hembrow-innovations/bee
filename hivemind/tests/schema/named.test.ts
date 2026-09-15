import assert from "node:assert/strict";
import { test } from "node:test";
import { namedAllowlist } from "../../src/schema/named.ts";

test("namedAllowlist returns the quarantine key set", () => {
  const keys = namedAllowlist("quarantine");
  assert.equal(keys.has("origin-location"), true);
  assert.equal(keys.has("quarantined-at"), true);
  assert.equal(keys.has("fault"), true);
  assert.equal(keys.size, 3);
});

test("namedAllowlist throws on an unknown folder schema", () => {
  assert.throws(
    () => namedAllowlist("ticket"),
    /Unknown folder schema "ticket"/,
  );
  assert.throws(
    () => namedAllowlist("planning"),
    /Unknown folder schema "planning"/,
  );
});
