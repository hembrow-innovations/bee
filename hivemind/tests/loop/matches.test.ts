import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import type {
  PipelineLane,
  UnitLane,
} from "../../src/config/loadConfig.ts";
import type {
  HistoryEvent,
  Journal,
} from "../../src/journal/journal.ts";
import type { Match } from "../../src/match/matcher.ts";
import {
  spawnMatches,
  type LiveRun,
} from "../../src/loop/matches.ts";

function unitLane(extra?: Partial<UnitLane>): UnitLane {
  return {
    type: "single",
    lane: "plan",
    trigger: { status: "ready-for-agent" },
    need: undefined,
    exclusive: [],
    claimStatus: "active",
    backoffMs: 0,
    cooldownMs: 0,
    ttlMs: 0,
    timeoutMs: 0,
    stallMs: 0,
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
    exclusive: [],
    claimStatus: "active",
    backoffMs: 0,
    cooldownMs: 0,
    ttlMs: 0,
    timeoutMs: 0,
    stallMs: 0,
    concurrency: 1,
    stages: [],
    ...extra,
  };
}

function liveRun(extra?: Partial<LiveRun>): LiveRun {
  return {
    exclusive: [],
    wait: Promise.resolve(0),
    kill: () => {},
    done: false,
    path: "tickets/one.md",
    lane: "build",
    runId: "run-live",
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

test("overlapping live exclusive sets skip without claim", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md", { exclusive: ["tickets"] })],
    env: {},
    live: [liveRun({ exclusive: ["tickets"] })],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "exclusive",
    },
  ]);
});

test("prefix-overlapping exclusive sets skip", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md", { exclusive: ["tickets/inbox"] })],
    env: {},
    live: [liveRun({ exclusive: ["tickets"] })],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.equal(events[0]?.kind, "skip");
  if (events[0]?.kind !== "skip") return;
  assert.equal(events[0].reason, "exclusive");
});

test("trailing slashes do not dodge exclusive overlap", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md", { exclusive: ["tickets/"] })],
    env: {},
    live: [liveRun({ exclusive: ["tickets"] })],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.equal(events[0]?.kind, "skip");
  if (events[0]?.kind !== "skip") return;
  assert.equal(events[0].reason, "exclusive");
});

test("disjoint exclusive sets still spawn", () => {
  const cwd = withTemp("hivemind-excl-ok-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [liveRun({ exclusive: ["docs"] })];
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md", { exclusive: ["tickets"] })],
    env: {},
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  assert.equal(spawned, 1);
  assert.equal(
    events.some(
      (event) => event.kind === "skip" && event.reason === "exclusive",
    ),
    false,
  );
  assert.equal(
    events.some((event) => event.kind === "claim"),
    true,
  );
});

test("spawnMatches skips a lane at its seat cap with reason concurrency", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md")],
    env: {},
    live: [liveRun({ lane: "plan", path: "tickets/one.md" })],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "concurrency",
    },
  ]);
});

test("spawnMatches skips a live path with reason live", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md")],
    env: {},
    live: [liveRun({ lane: "build", path: "tickets/two.md" })],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "live",
    },
  ]);
});

test("a not-done live run with a dead pid does not block a second spawn on concurrency 1", () => {
  const cwd = withTemp("hivemind-dead-pid-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [
    liveRun({
      lane: "plan",
      path: "tickets/one.md",
      pid: deadPid(),
      wait: new Promise(() => {}),
    }),
  ];
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: {},
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  assert.equal(spawned, 1);
  assert.equal(live[0]?.done, true);
  assert.equal(
    events.some(
      (event) => event.kind === "skip" && event.reason === "concurrency",
    ),
    false,
  );
  assert.equal(
    events.some((event) => event.kind === "claim"),
    true,
  );
});

test("a not-done live run with no pid still blocks", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md")],
    env: {},
    live: [
      liveRun({
        lane: "plan",
        path: "tickets/one.md",
        wait: new Promise(() => {}),
      }),
    ],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "concurrency",
    },
  ]);
});

test("a not-done live run with a dead pid does not skip the same path as live", () => {
  const cwd = withTemp("hivemind-dead-path-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  const { journal, events } = memoryJournal();
  const live: LiveRun[] = [
    liveRun({
      lane: "build",
      path: "tickets/two.md",
      pid: deadPid(),
      wait: new Promise(() => {}),
    }),
  ];
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: {},
    live,
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  assert.equal(spawned, 1);
  assert.equal(live[0]?.done, true);
  assert.equal(
    events.some((event) => event.kind === "skip" && event.reason === "live"),
    false,
  );
  assert.equal(
    events.some((event) => event.kind === "claim"),
    true,
  );
});

test("spawnMatches skips a lane in cooldown with reason cooldown", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md", { cooldownMs: 60_000 })],
    env: {},
    live: [],
    journal,
    lastFinished: new Map([["plan", 1_000]]),
    now: () => 1_500,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "cooldown",
    },
  ]);
});

test("spawnMatches skips a lost claim with reason claim-race", () => {
  const cwd = withTemp("hivemind-claim-race-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: active\n---\n",
  );
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: {},
    live: [],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "claim-race",
    },
  ]);
});

test("spawnMatches skips a missing prompt with reason missing-prompt", () => {
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd: "/tmp/unused",
    matches: [matchFor("tickets/two.md", { prompt: "missing.md" })],
    env: {},
    live: [],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "missing-prompt",
    },
  ]);
});

test("spawnMatches copies first child pid onto the live run", () => {
  const cwd = withTemp("hivemind-live-pid-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  const live: LiveRun[] = [];
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: {},
    live,
    spawnChild: () => ({
      wait: Promise.resolve(0),
      kill: () => {},
      pid: 4242,
    }),
  });
  assert.equal(spawned, 1);
  assert.equal(live[0]?.pid, 4242);
});

test("hivemind.spawn:stage-path spawnMatches interpolates stage and path", () => {
  const cwd = withTemp("hivemind-stage-path-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  const spawnedArgv: string[][] = [];
  const spawned = spawnMatches({
    cwd,
    matches: [
      {
        lane: pipelineLane({
          stages: [
            {
              stage: "plan",
              cmd: ["/bin/echo", "{{stage}}", "{{path}}"],
              agent: undefined,
              prompt: undefined,
              exclusive: [],
              claimStatus: "active",
              scalars: {},
            },
          ],
        }),
        note: {
          path: "tickets/two.md",
          frontMatter: { status: "ready-for-agent" },
        },
      },
    ],
    env: {},
    live: [],
    spawnChild: (opts) => {
      spawnedArgv.push([...opts.argv]);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(spawned, 1);
  assert.deepEqual(spawnedArgv[0], ["/bin/echo", "plan", "tickets/two.md"]);
});

test("spawnMatches interpolates minted run-id into dest cmd", () => {
  const cwd = withTemp("hivemind-run-id-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  const spawnedArgv: string[][] = [];
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md", { cmd: ["/bin/echo", "{{run-id}}"] })],
    env: {},
    live: [],
    spawnChild: (opts) => {
      spawnedArgv.push([...opts.argv]);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(spawned, 1);
  const argv = spawnedArgv[0];
  assert.equal(argv?.[0], "/bin/echo");
  const runId = argv?.[1];
  assert.match(
    runId ?? "",
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
  assert.match(
    readFileSync(join(cwd, "tickets", "two.md"), "utf8"),
    new RegExp(`^claimed-by: ${runId}$`, "m"),
  );
});

test("spawnMatches child env reuses minted run-id as HIVEMIND_RUN_ID", () => {
  const cwd = withTemp("hivemind-child-env-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  let childEnv: NodeJS.ProcessEnv | undefined;
  const spawned = spawnMatches({
    cwd,
    matches: [matchFor("tickets/two.md")],
    env: { KEEP: "yes" },
    live: [],
    spawnChild: (opts) => {
      childEnv = opts.env;
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(spawned, 1);
  const runId = childEnv?.HIVEMIND_RUN_ID ?? "";
  assert.match(
    runId,
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
  assert.match(
    readFileSync(join(cwd, "tickets", "two.md"), "utf8"),
    new RegExp(`^claimed-by: ${runId}$`, "m"),
  );
  assert.equal(childEnv?.HIVEMIND_LANE, "plan");
  assert.equal(childEnv?.HIVEMIND_PATH, "tickets/two.md");
  assert.equal(childEnv?.KEEP, "yes");
});

test("spawnMatches skips unknown placeholder with no claim", () => {
  const cwd = withTemp("hivemind-unknown-");
  mkdirSync(join(cwd, "tickets"));
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n",
  );
  const { journal, events } = memoryJournal();
  const spawned = spawnMatches({
    cwd,
    matches: [
      matchFor("tickets/two.md", { cmd: ["/bin/echo", "{{mystery}}"] }),
    ],
    env: {},
    live: [],
    journal,
    spawnChild: () => {
      throw new Error("must not spawn");
    },
  });
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/two.md",
      reason: "cmd-skip",
    },
  ]);
  assert.match(
    readFileSync(join(cwd, "tickets", "two.md"), "utf8"),
    /^status: ready-for-agent$/m,
  );
  assert.equal(
    readFileSync(join(cwd, "tickets", "two.md"), "utf8").includes(
      "claimed-by:",
    ),
    false,
  );
});

function deadPid(): number {
  const child = spawnSync(process.execPath, ["-e", "process.exit(0)"]);
  const pid = child.pid;
  assert.equal(typeof pid, "number");
  if (typeof pid !== "number") throw new Error("expected pid");
  assert.notEqual(pid, 1);
  let gone = false;
  try {
    process.kill(pid, 0);
  } catch (error) {
    gone =
      typeof error === "object" &&
      error !== null &&
      "code" in error &&
      error.code === "ESRCH";
  }
  assert.equal(gone, true);
  return pid;
}
