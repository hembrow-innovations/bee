import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import type { SpawnSpec } from "../../src/config/loadConfig.ts";
import type {
  HistoryEvent,
  Journal,
} from "../../src/journal/journal.ts";
import {
  renderArgv,
  startSpawn,
  type SpawnChild,
} from "../../src/spawn/spawner.ts";

function spec(cmd: SpawnSpec["cmd"], extra?: Partial<SpawnSpec>): SpawnSpec {
  return {
    cmd,
    agent: undefined,
    prompt: undefined,
    exclusive: [],
    claimStatus: "active",
    scalars: {},
    ...extra,
  };
}

function memoryJournal(): { journal: Journal; events: HistoryEvent[] } {
  const events: HistoryEvent[] = [];
  return {
    events,
    journal: {
      record(event) {
        events.push(event);
      },
    },
  };
}

test("renderArgv tokenizes a string cmd after interpolation", () => {
  const result = renderArgv({
    specs: [spec("/bin/echo {{cwd}} {{lane}}")],
    cwd: "/tmp/project",
    lane: "plan",
    env: {},
  });
  assert.deepEqual(result, {
    kind: "ok",
    argvList: [["/bin/echo", "/tmp/project", "plan"]],
  });
});

test("renderArgv keeps spaces inside a list cmd part", () => {
  const result = renderArgv({
    specs: [
      spec(["/bin/echo", "hello world", "{{exclusive}}"], {
        exclusive: ["tickets/", "docs/"],
      }),
    ],
    cwd: "/tmp/project",
    lane: "plan",
    env: {},
  });
  assert.deepEqual(result, {
    kind: "ok",
    argvList: [["/bin/echo", "hello world", "tickets/ docs/"]],
  });
});

test("renderArgv skips when the prompt file is missing", () => {
  const cwd = withTemp("hivemind-spawn-prompt-");
  const result = renderArgv({
    specs: [spec("/bin/echo hi", { prompt: "missing.md" })],
    cwd,
    lane: "plan",
    env: {},
  });
  assert.deepEqual(result, { kind: "skip", reason: "missing-prompt" });
});

test("renderArgv skips when env interpolation cannot resolve", () => {
  const result = renderArgv({
    specs: [spec("/bin/echo {{env.MISSING}}")],
    cwd: "/tmp/project",
    lane: "plan",
    env: {},
  });
  assert.deepEqual(result, { kind: "skip", reason: "cmd-skip" });
});

test("renderArgv skips unmatched quotes", () => {
  const result = renderArgv({
    specs: [spec('/bin/echo "hello')],
    cwd: "/tmp/project",
    lane: "plan",
    env: {},
  });
  assert.deepEqual(result, { kind: "skip", reason: "cmd-skip" });
});

test("renderArgv skips the pipeline when a later stage cannot render", () => {
  const result = renderArgv({
    specs: [spec("/bin/echo one"), spec("/bin/echo {{env.MISSING}}")],
    cwd: "/tmp/project",
    lane: "workflow",
    env: {},
  });
  assert.deepEqual(result, { kind: "skip", reason: "cmd-skip" });
});

test("{{run-id}} interpolates", () => {
  const result = renderArgv({
    specs: [spec("/bin/echo {{run-id}}")],
    cwd: "/tmp/project",
    lane: "plan",
    env: {},
    runId: "11111111-1111-1111-1111-111111111111",
  });
  assert.deepEqual(result, {
    kind: "ok",
    argvList: [["/bin/echo", "11111111-1111-1111-1111-111111111111"]],
  });
});

test("hivemind.spawn:stage-path renderArgv interpolates stage and path", () => {
  const result = renderArgv({
    specs: [spec("/bin/echo {{stage}}"), spec("/bin/echo {{path}} {{stage}}")],
    cwd: "/tmp/project",
    lane: "workflow",
    env: {},
    path: "tickets/two.md",
    stages: ["plan", "build"],
  });
  assert.deepEqual(result, {
    kind: "ok",
    argvList: [
      ["/bin/echo", "plan"],
      ["/bin/echo", "tickets/two.md", "build"],
    ],
  });
});

test("renderArgv interpolates an existing prompt path", () => {
  const cwd = withTemp("hivemind-spawn-prompt-ok-");
  mkdirSync(join(cwd, "prompts"));
  writeFileSync(join(cwd, "prompts", "agent.md"), "# hi\n");
  const result = renderArgv({
    specs: [spec("/bin/echo {{prompt}}", { prompt: "prompts/agent.md" })],
    cwd,
    lane: "plan",
    env: {},
  });
  assert.deepEqual(result, {
    kind: "ok",
    argvList: [["/bin/echo", "prompts/agent.md"]],
  });
});

test("child env has HIVEMIND_RUN_ID", async () => {
  let childEnv: NodeJS.ProcessEnv | undefined;
  const handle = startSpawn({
    argvList: [["echo"]],
    cwd: "/tmp/project",
    env: { KEEP: "yes" },
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
    spawnChild: (opts) => {
      childEnv = opts.env;
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(await handle.wait, 0);
  assert.equal(childEnv?.HIVEMIND_RUN_ID, "run-1");
  assert.equal(childEnv?.HIVEMIND_LANE, "plan");
  assert.equal(childEnv?.HIVEMIND_PATH, "tickets/agent.md");
  assert.equal(childEnv?.KEEP, "yes");
});

test("startSpawn records spawn then exit for one child", async () => {
  const { journal, events } = memoryJournal();
  const handle = startSpawn({
    argvList: [["echo"]],
    cwd: "/tmp/project",
    env: {},
    journal,
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  assert.equal(await handle.wait, 0);
  assert.deepEqual(events, [
    {
      kind: "spawn",
      lane: "plan",
      path: "tickets/agent.md",
      runId: "run-1",
    },
    {
      kind: "exit",
      lane: "plan",
      path: "tickets/agent.md",
      runId: "run-1",
      status: 0,
    },
  ]);
});

test("startSpawn spawn/exit carry stage and pid", async () => {
  const { journal, events } = memoryJournal();
  const handle = startSpawn({
    argvList: [["echo"]],
    cwd: "/tmp/project",
    env: {},
    journal,
    lane: "workflow",
    path: "tickets/agent.md",
    runId: "run-1",
    stages: ["plan"],
    spawnChild: () => ({
      wait: Promise.resolve(0),
      kill: () => {},
      pid: 8122,
    }),
  });
  assert.equal(await handle.wait, 0);
  assert.deepEqual(events, [
    {
      kind: "spawn",
      lane: "workflow",
      path: "tickets/agent.md",
      runId: "run-1",
      stage: "plan",
      pid: 8122,
    },
    {
      kind: "exit",
      lane: "workflow",
      path: "tickets/agent.md",
      runId: "run-1",
      status: 0,
      stage: "plan",
      pid: 8122,
    },
  ]);
});

test("startSpawn exposes first child pid before wait resolves", () => {
  const handle = startSpawn({
    argvList: [["echo"]],
    cwd: "/tmp/project",
    env: {},
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
    spawnChild: () => ({
      wait: new Promise(() => {}),
      kill: () => {},
      pid: 4242,
    }),
  });
  assert.equal(handle.pid, 4242);
});

test("startSpawn waits for each stage before starting the next", async () => {
  const calls: string[] = [];
  let releaseFirst: ((status: number) => void) | undefined;
  const firstWait = new Promise<number>((resolve) => {
    releaseFirst = resolve;
  });
  const spawnChild: SpawnChild = (opts) => {
    const name = opts.argv[0];
    if (name === undefined) throw new Error("expected argv");
    calls.push(name);
    if (name === "one") {
      return { wait: firstWait, kill: () => {} };
    }
    return { wait: Promise.resolve(0), kill: () => {} };
  };
  const handle = startSpawn({
    argvList: [["one"], ["two"]],
    cwd: "/tmp/project",
    env: {},
    lane: "workflow",
    path: "tickets/agent.md",
    runId: "run-1",
    spawnChild,
  });
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, ["one"]);
  assert.equal(releaseFirst === undefined, false);
  releaseFirst?.(0);
  assert.equal(await handle.wait, 0);
  assert.deepEqual(calls, ["one", "two"]);
});

test("hivemind.spawn:timeout-stall elapsed timeout kills the child and journals timeout", async () => {
  const { journal, events } = memoryJournal();
  let killed = 0;
  let resolveWait: ((status: number) => void) | undefined;
  const handle = startSpawn({
    argvList: [["hang"]],
    cwd: "/tmp/project",
    env: {},
    journal,
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-timeout",
    timeoutMs: 30,
    spawnChild: () => ({
      wait: new Promise<number>((resolve) => {
        resolveWait = resolve;
      }),
      kill: () => {
        killed += 1;
        resolveWait?.(1);
      },
    }),
  });
  assert.equal(await handle.wait, 1);
  assert.equal(killed, 1);
  const exit = events.find((event) => event.kind === "exit");
  assert.equal(exit?.kind, "exit");
  if (exit?.kind !== "exit") throw new Error("expected exit");
  assert.equal(exit.status, 1);
  assert.equal(exit.reason, "timeout");
});

test("hivemind.spawn:timeout-stall stall kills the child and journals stall", async () => {
  const { journal, events } = memoryJournal();
  let killed = 0;
  let resolveWait: ((status: number) => void) | undefined;
  const handle = startSpawn({
    argvList: [["hang"]],
    cwd: "/tmp/project",
    env: {},
    journal,
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-stall",
    stallMs: 30,
    spawnChild: () => ({
      wait: new Promise<number>((resolve) => {
        resolveWait = resolve;
      }),
      kill: () => {
        killed += 1;
        resolveWait?.(1);
      },
    }),
  });
  assert.equal(await handle.wait, 1);
  assert.equal(killed, 1);
  const exit = events.find((event) => event.kind === "exit");
  assert.equal(exit?.kind, "exit");
  if (exit?.kind !== "exit") throw new Error("expected exit");
  assert.equal(exit.status, 1);
  assert.equal(exit.reason, "stall");
});

test("startSpawn stops a pipeline on nonzero and journals each started stage", async () => {
  const { journal, events } = memoryJournal();
  const calls: string[] = [];
  const handle = startSpawn({
    argvList: [["one"], ["two"], ["three"]],
    cwd: "/tmp/project",
    env: {},
    journal,
    lane: "workflow",
    path: "tickets/agent.md",
    runId: "run-2",
    spawnChild: (opts) => {
      const name = opts.argv[0];
      if (name === undefined) throw new Error("expected argv");
      calls.push(name);
      return {
        wait: Promise.resolve(name === "two" ? 1 : 0),
        kill: () => {},
      };
    },
  });
  assert.equal(await handle.wait, 1);
  assert.deepEqual(calls, ["one", "two"]);
  assert.deepEqual(
    events.map((event) => {
      if (event.kind === "spawn") return `spawn`;
      if (event.kind === "exit") return `exit:${event.status}`;
      return event.kind;
    }),
    ["spawn", "exit:0", "spawn", "exit:1"],
  );
});
