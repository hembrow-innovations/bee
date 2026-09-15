import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";
import { test } from "node:test";
import type {
  PipelineLane,
  UnitLane,
} from "../../src/config/loadConfig.ts";
import type {
  HistoryEvent,
  Journal,
} from "../../src/journal/journal.ts";
import {
  type LiveRun,
  spawnMatches,
} from "../../src/loop/matches.ts";
import type { Match } from "../../src/match/matcher.ts";
import { startSpawn } from "../../src/spawn/spawner.ts";
import { withTemp } from "../with-temp.ts";

function unitLane(extra?: Partial<UnitLane>): UnitLane {
  return {
    type: "single",
    lane: "plan",
    trigger: { status: "ready-for-agent" },
    need: undefined,
    exclusive: ["apps"],
    claimStatus: "active",
    backoffMs: 0,
    cooldownMs: 0,
    ttlMs: 0,
    timeoutMs: 0,
    stallMs: 0,
    audit: "git-diff",
    concurrency: 1,
    cmd: ["/bin/echo"],
    agent: undefined,
    prompt: undefined,
    scalars: {},
    ...extra,
  };
}

function pipelineLane(extra?: Partial<PipelineLane>): PipelineLane {
  return {
    type: "pipeline",
    lane: "workflow",
    trigger: { status: "ready-for-agent" },
    need: undefined,
    exclusive: ["apps"],
    claimStatus: "active",
    backoffMs: 0,
    cooldownMs: 0,
    ttlMs: 0,
    timeoutMs: 0,
    stallMs: 0,
    audit: "git-diff",
    concurrency: 1,
    stages: [
      {
        stage: "one",
        cmd: ["/bin/echo", "one"],
        agent: undefined,
        prompt: undefined,
        exclusive: [],
        claimStatus: "active",
        scalars: {},
      },
      {
        stage: "two",
        cmd: ["/bin/echo", "two"],
        agent: undefined,
        prompt: undefined,
        exclusive: [],
        claimStatus: "active",
        scalars: {},
      },
    ],
    ...extra,
  };
}

function matchFor(path: string, extra?: Partial<UnitLane>): Match {
  return {
    lane: unitLane(extra),
    note: { path, frontMatter: { status: "ready-for-agent" } },
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

function writeTicket(cwd: string): void {
  mkdirSync(join(cwd, "tickets"), { recursive: true });
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
}

function writeGitStub(bin: string, body: string): void {
  mkdirSync(bin, { recursive: true });
  const file = join(bin, "git");
  writeFileSync(file, `#!/usr/bin/env node\n${body}\n`);
  chmodSync(file, 0o755);
}

function gitEnv(bin: string): NodeJS.ProcessEnv {
  return {
    ...process.env,
    PATH: `${bin}${delimiter}${process.env.PATH ?? ""}`,
  };
}

function skipReasons(events: readonly HistoryEvent[]): string[] {
  return events
    .filter((event) => event.kind === "skip")
    .map((event) => (event.kind === "skip" ? event.reason : ""));
}

function claimed(cwd: string): string {
  return readFileSync(join(cwd, "tickets", "two.md"), "utf8");
}

function assertClaimLeft(cwd: string): void {
  const raw = claimed(cwd);
  assert.match(raw, /^status: active$/m);
  assert.match(raw, /^claimed-by: /m);
}

async function afterWait(live: readonly LiveRun[]): Promise<void> {
  await Promise.all(live.map((run) => run.wait));
}

test("hivemind.loop:post-exit-scope", async () => {
  const cwd = withTemp("hivemind-post-exit-scope-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  const stamp = join(cwd, "git-calls");
  writeGitStub(
    bin,
    [
      `import { appendFileSync } from "node:fs";`,
      `appendFileSync(${JSON.stringify(stamp)}, "1");`,
      `process.stdout.write("other/file.ts\\n");`,
    ].join("\n"),
  );
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  assert.equal(spawned, 1);
  await afterWait(live);
  assert.deepEqual(skipReasons(events), ["scope-breach"]);
  assertClaimLeft(cwd);
  assert.equal(readFileSync(stamp, "utf8"), "1");
  console.log("hivemind.loop:post-exit-scope");
});

test("timeout kill still journals scope-breach and leaves the claim", async () => {
  const cwd = withTemp("hivemind-post-exit-timeout-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  writeGitStub(bin, `process.stdout.write("leak.ts\\n");`);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md", { timeoutMs: 30 })],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => {
      let resolveWait: ((status: number) => void) | undefined;
      return {
        wait: new Promise<number>((resolve) => {
          resolveWait = resolve;
        }),
        kill: () => {
          resolveWait?.(1);
        },
      };
    },
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), ["scope-breach"]);
  assertClaimLeft(cwd);
});

test("stall kill still journals scope-breach and leaves the claim", async () => {
  const cwd = withTemp("hivemind-post-exit-stall-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  writeGitStub(bin, `process.stdout.write("leak.ts\\n");`);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md", { stallMs: 30 })],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => {
      let resolveWait: ((status: number) => void) | undefined;
      return {
        wait: new Promise<number>((resolve) => {
          resolveWait = resolve;
        }),
        kill: () => {
          resolveWait?.(1);
        },
      };
    },
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), ["scope-breach"]);
  assertClaimLeft(cwd);
});

test("pipeline chain runs git once after wait", async () => {
  const cwd = withTemp("hivemind-post-exit-pipeline-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  const stamp = join(cwd, "git-calls");
  writeGitStub(
    bin,
    [
      `import { appendFileSync } from "node:fs";`,
      `appendFileSync(${JSON.stringify(stamp)}, "1");`,
      `process.stdout.write("other/file.ts\\n");`,
    ].join("\n"),
  );
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  let children = 0;
  const spawned = spawnMatches({
    cwd,
    matches: [
      {
        lane: pipelineLane(),
        note: {
          path: "tickets/two.md",
          frontMatter: { status: "ready-for-agent" },
        },
      },
    ],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => {
      children += 1;
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(spawned, 1);
  await afterWait(live);
  assert.equal(children, 2);
  assert.equal(readFileSync(stamp, "utf8"), "1");
  assert.deepEqual(skipReasons(events), ["scope-breach"]);
  assertClaimLeft(cwd);
});

test("omit audit does not run git and does not skip", async () => {
  const cwd = withTemp("hivemind-post-exit-omit-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  const stamp = join(cwd, "git-calls");
  writeGitStub(
    bin,
    [
      `import { appendFileSync } from "node:fs";`,
      `appendFileSync(${JSON.stringify(stamp)}, "1");`,
      `process.stdout.write("other/file.ts\\n");`,
    ].join("\n"),
  );
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md", { audit: undefined })],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), []);
  assertClaimLeft(cwd);
  assert.equal(
    (() => {
      try {
        return readFileSync(stamp, "utf8");
      } catch {
        return "";
      }
    })(),
    "",
  );
});

test("git missing does not skip", async () => {
  const cwd = withTemp("hivemind-post-exit-git-missing-");
  writeTicket(cwd);
  const empty = join(cwd, "empty-bin");
  mkdirSync(empty);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: { ...process.env, PATH: empty },
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), []);
  assertClaimLeft(cwd);
});

test("git fail does not skip", async () => {
  const cwd = withTemp("hivemind-post-exit-git-fail-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  writeGitStub(bin, `process.exit(1);`);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), []);
  assertClaimLeft(cwd);
});

test("empty names do not skip", async () => {
  const cwd = withTemp("hivemind-post-exit-empty-names-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  writeGitStub(bin, `process.stdout.write("");`);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), []);
  assertClaimLeft(cwd);
});

test("empty opted-in scope treats every listed name as a breach", async () => {
  const cwd = withTemp("hivemind-post-exit-empty-scope-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  writeGitStub(bin, `process.stdout.write("apps/foo.ts\\n");`);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md", { exclusive: [] })],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), ["scope-breach"]);
  assertClaimLeft(cwd);
});

test("in-scope name does not skip", async () => {
  const cwd = withTemp("hivemind-post-exit-in-scope-");
  writeTicket(cwd);
  const bin = join(cwd, "bin");
  writeGitStub(bin, `process.stdout.write("apps/foo.ts\\n");`);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: gitEnv(bin),
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), []);
  assertClaimLeft(cwd);
});

test("untracked names stay out", async () => {
  const cwd = withTemp("hivemind-post-exit-untracked-");
  writeTicket(cwd);
  writeFileSync(join(cwd, "secret.ts"), "leak\n");
  const git = (args: string[]) => {
    const proc = spawnSync("git", args, { cwd, encoding: "utf8" });
    assert.equal(proc.status, 0, proc.stderr);
  };
  git(["init"]);
  git(["config", "user.email", "t@t.t"]);
  git(["config", "user.name", "t"]);
  writeFileSync(join(cwd, "kept.ts"), "ok\n");
  git(["add", "kept.ts"]);
  git(["commit", "-m", "init"]);
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [];
  spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: process.env,
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  await afterWait(live);
  assert.deepEqual(skipReasons(events), []);
  assertClaimLeft(cwd);
});

test("startSpawn has no git", async () => {
  const cwd = withTemp("hivemind-post-exit-startspawn-");
  const bin = join(cwd, "bin");
  const stamp = join(cwd, "git-calls");
  writeGitStub(
    bin,
    [
      `import { appendFileSync } from "node:fs";`,
      `appendFileSync(${JSON.stringify(stamp)}, "1");`,
    ].join("\n"),
  );
  const handle = startSpawn({
    argvList: [["echo"]],
    cwd,
    env: gitEnv(bin),
    lane: "plan",
    path: "tickets/two.md",
    runId: "run-1",
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  assert.equal(await handle.wait, 0);
  assert.equal(
    (() => {
      try {
        return readFileSync(stamp, "utf8");
      } catch {
        return "";
      }
    })(),
    "",
  );
});
