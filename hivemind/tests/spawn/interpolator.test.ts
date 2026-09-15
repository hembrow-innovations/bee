import assert from "node:assert/strict";
import { test } from "node:test";
import type { SpawnSpec } from "../../src/config/loadConfig.ts";
import { interpolate } from "../../src/spawn/interpolator.ts";

function spec(extra?: Partial<SpawnSpec>): SpawnSpec {
  return {
    cmd: "/bin/echo",
    agent: undefined,
    prompt: undefined,
    exclusive: [],
    claimStatus: "active",
    scalars: {},
    ...extra,
  };
}

test("interpolate skips missing or empty env", () => {
  const missing = interpolate({
    template: "/bin/echo {{env.SECRET}}",
    cwd: "/tmp/project",
    lane: "plan",
    spec: spec(),
    env: {},
  });
  const empty = interpolate({
    template: "/bin/echo {{env.SECRET}}",
    cwd: "/tmp/project",
    lane: "plan",
    spec: spec(),
    env: { SECRET: "" },
  });
  assert.deepEqual(missing, { kind: "skip" });
  assert.deepEqual(empty, { kind: "skip" });
});

test("interpolate skips leftover {{", () => {
  const result = interpolate({
    template: "/bin/echo {{cwd}} {{",
    cwd: "/tmp/project",
    lane: "plan",
    spec: spec(),
    env: {},
  });
  assert.deepEqual(result, { kind: "skip" });
});

test("interpolate skips an unknown placeholder", () => {
  const result = interpolate({
    template: "/bin/echo {{mystery}}",
    cwd: "/tmp/project",
    lane: "plan",
    spec: spec(),
    env: {},
  });
  assert.deepEqual(result, { kind: "skip" });
});

test("interpolate substitutes run-id", () => {
  const result = interpolate({
    template: "/bin/echo {{run-id}}",
    cwd: "/tmp/project",
    lane: "plan",
    spec: spec(),
    env: {},
    runId: "11111111-1111-1111-1111-111111111111",
  });
  assert.deepEqual(result, {
    kind: "ok",
    value: "/bin/echo 11111111-1111-1111-1111-111111111111",
  });
});

test("hivemind.spawn:stage-path", () => {
  const resolved = interpolate({
    template: "/bin/echo {{stage}} {{path}}",
    cwd: "/tmp/project",
    lane: "workflow",
    spec: spec(),
    env: {},
    stage: "plan",
    path: "tickets/two.md",
  });
  assert.deepEqual(resolved, {
    kind: "ok",
    value: "/bin/echo plan tickets/two.md",
  });
  const unknown = interpolate({
    template: "/bin/echo {{mystery}}",
    cwd: "/tmp/project",
    lane: "workflow",
    spec: spec(),
    env: {},
    stage: "plan",
    path: "tickets/two.md",
  });
  assert.deepEqual(unknown, { kind: "skip" });
  const missing = interpolate({
    template: "/bin/echo {{stage}} {{path}} {{id}}",
    cwd: "/tmp/project",
    lane: "plan",
    spec: spec(),
    env: {},
  });
  assert.deepEqual(missing, { kind: "skip" });
});
