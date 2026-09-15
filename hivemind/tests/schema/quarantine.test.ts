import assert from "node:assert/strict";
import { test } from "node:test";
import { QUARANTINE_KEYS } from "../../src/schema/quarantine.ts";

test("QUARANTINE_KEYS is origin-location, quarantined-at, and fault", () => {
  assert.deepEqual(
    [...QUARANTINE_KEYS],
    ["origin-location", "quarantined-at", "fault"],
  );
});
