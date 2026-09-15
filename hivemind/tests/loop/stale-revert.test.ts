import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { withTemp } from "../with-temp.ts";
import { loadConfig } from "../../src/config/loadConfig.ts";
import {
  createJournal,
  resolveHistory,
} from "../../src/journal/journal.ts";
import { runTick } from "../../src/loop/tick.ts";

const CLI = fileURLToPath(
  new URL("../../src/cli.ts", import.meta.url),
);

const BODY = "# keep this body\n";
const TTL_MS = 1_800_000;

type Proc = { status: number | null; stdout: string; stderr: string };

test("hivemind.loop:stale-revert: after spawn, elapsed ttl with no live Run reverts the claim", () => {
  const cwd = setupTtlProject({ ttl: "30m" });
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(claimed, /^status: active$/m);
  const runId = claimedBy(claimed);
  assert.equal(claimed.includes(BODY.trimEnd()), true);

  backdateFirstSpawn(join(cwd, "hivemind.tsv"), TTL_MS + 60_000);

  const second = once(cwd);
  assert.equal(second.status, 0, second.stderr);
  const reverted = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(reverted, /^status: ready-for-agent$/m);
  assert.equal(/^claimed-by:/m.test(reverted), false);
  assert.equal(reverted.includes(BODY.trimEnd()), true);
  assert.match(second.stderr, /hivemind revert plan tickets\/agent\.md/);
  assert.equal(
    second.stderr.includes("hivemind claim plan tickets/agent.md"),
    false,
  );

  const revertRow = tsvRows(join(cwd, "hivemind.tsv")).find(
    (row) => row[1] === "revert",
  );
  assert.notEqual(revertRow, undefined);
  assert.equal(revertRow?.[2], "plan");
  assert.equal(revertRow?.[3], "tickets/agent.md");
  assert.equal(revertRow?.[4], runId);
});

test("hivemind.loop:stale-live-skip: live Run past TTL stays claimed with no revert row", () => {
  const cwd = setupTtlProject({ ttl: "30m" });
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  const runId = claimedBy(claimed);
  backdateFirstSpawn(join(cwd, "hivemind.tsv"), TTL_MS + 60_000);

  const config = loadConfig(cwd);
  const lines: string[] = [];
  const journal = createJournal({
    historyPath: resolveHistory({ cwd, path: config.history }),
    writeLine: (line) => {
      lines.push(line);
    },
  });
  runTick({
    cwd,
    config,
    lanes: config.lanes,
    env: process.env,
    live: [
      {
        exclusive: [],
        wait: new Promise(() => {}),
        kill: () => {
          throw new Error("must not kill");
        },
        done: false,
        path: "tickets/agent.md",
        lane: "plan",
        runId,
      },
    ],
    journal,
  });

  const still = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(still, /^status: active$/m);
  assert.match(still, new RegExp(`^claimed-by: ${runId}$`, "m"));
  assert.equal(
    lines.some((line) => line.includes("hivemind revert")),
    false,
  );
  assert.equal(
    lines.some((line) => line.includes("hivemind skip")),
    false,
  );
  assert.equal(
    tsvRows(join(cwd, "hivemind.tsv")).some((row) => row[1] === "revert"),
    false,
  );
});

test("hivemind.loop:stale-dead-pid: not-done live run with a dead pid is not live", () => {
  const cwd = setupTtlProject({ ttl: "30m" });
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  const runId = claimedBy(claimed);
  backdateFirstSpawn(join(cwd, "hivemind.tsv"), TTL_MS + 60_000);

  const config = loadConfig(cwd);
  const lines: string[] = [];
  const journal = createJournal({
    historyPath: resolveHistory({ cwd, path: config.history }),
    writeLine: (line) => {
      lines.push(line);
    },
  });
  runTick({
    cwd,
    config,
    lanes: config.lanes,
    env: process.env,
    live: [
      {
        exclusive: [],
        wait: new Promise(() => {}),
        kill: () => {
          throw new Error("must not kill");
        },
        done: false,
        path: "tickets/agent.md",
        lane: "plan",
        runId,
        pid: deadPid(),
      },
    ],
    journal,
  });

  const reverted = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(reverted, /^status: ready-for-agent$/m);
  assert.equal(/^claimed-by:/m.test(reverted), false);
  assert.equal(
    lines.some((line) => line.includes("hivemind revert")),
    true,
  );
  assert.equal(
    tsvRows(join(cwd, "hivemind.tsv")).some((row) => row[1] === "revert"),
    true,
  );
});

test("hivemind.config:lane-ttl-omit: omitted ttl and ttl 0 leave a dead claim claimed after the same wait", () => {
  for (const ttl of [undefined, "0"] as const) {
    const cwd = setupTtlProject({ ttl });
    writeTicket(cwd, "agent.md", "ready-for-agent");
    const first = once(cwd);
    assert.equal(first.status, 0, first.stderr);
    const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
    const runId = claimedBy(claimed);
    backdateFirstSpawn(join(cwd, "hivemind.tsv"), TTL_MS + 60_000);
    const second = once(cwd);
    assert.equal(second.status, 0, second.stderr);
    const still = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
    assert.match(still, /^status: active$/m);
    assert.match(still, new RegExp(`^claimed-by: ${runId}$`, "m"));
    assert.equal(second.stderr.includes("hivemind revert"), false);
    assert.equal(
      tsvRows(join(cwd, "hivemind.tsv")).some((row) => row[1] === "revert"),
      false,
    );
  }
});

test("hivemind.loop:once-revert-next-tick: the once that reverts does not claim that path again; a later once can", () => {
  const cwd = setupTtlProject({ ttl: "30m" });
  writeTicket(cwd, "agent.md", "ready-for-agent");
  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  backdateFirstSpawn(join(cwd, "hivemind.tsv"), TTL_MS + 60_000);

  const reverting = once(cwd);
  assert.equal(reverting.status, 0, reverting.stderr);
  assert.match(reverting.stderr, /hivemind revert plan tickets\/agent\.md/);
  assert.equal(
    reverting.stderr.includes("hivemind claim plan tickets/agent.md"),
    false,
  );
  assert.equal(
    reverting.stderr.includes("hivemind spawn plan tickets/agent.md"),
    false,
  );
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    /^status: ready-for-agent$/m,
  );

  const later = once(cwd);
  assert.equal(later.status, 0, later.stderr);
  assert.match(later.stderr, /hivemind claim plan tickets\/agent\.md/);
  assert.match(later.stderr, /hivemind spawn plan tickets\/agent\.md/);
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    /^status: active$/m,
  );
});

test("hivemind.loop:claimed-at-revert: elapsed claimed-at with no spawn row reverts the claim", () => {
  const cwd = setupTtlProject({ ttl: "30m" });
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(claimed, /^status: active$/m);
  const runId = claimedBy(claimed);
  assert.match(claimed, /^claimed-at: /m);
  assert.equal(claimed.includes(BODY.trimEnd()), true);

  dropSpawnRows(join(cwd, "hivemind.tsv"));
  backdateClaimedAt(join(cwd, "tickets", "agent.md"), TTL_MS + 60_000);

  const second = once(cwd);
  assert.equal(second.status, 0, second.stderr);
  const reverted = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(reverted, /^status: ready-for-agent$/m);
  assert.equal(/^claimed-by:/m.test(reverted), false);
  assert.equal(/^claimed-at:/m.test(reverted), false);
  assert.equal(reverted.includes(BODY.trimEnd()), true);
  assert.match(second.stderr, /hivemind revert plan tickets\/agent\.md/);
  assert.equal(
    second.stderr.includes("hivemind claim plan tickets/agent.md"),
    false,
  );

  const revertRow = tsvRows(join(cwd, "hivemind.tsv")).find(
    (row) => row[1] === "revert",
  );
  assert.notEqual(revertRow, undefined);
  assert.equal(revertRow?.[2], "plan");
  assert.equal(revertRow?.[3], "tickets/agent.md");
  assert.equal(revertRow?.[4], runId);
});

test("hivemind.loop:claimed-at-revert: live Run stays claimed when claimed-at elapsed and history has no spawn row", () => {
  const cwd = setupTtlProject({ ttl: "30m" });
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  const runId = claimedBy(claimed);
  dropSpawnRows(join(cwd, "hivemind.tsv"));
  backdateClaimedAt(join(cwd, "tickets", "agent.md"), TTL_MS + 60_000);

  const config = loadConfig(cwd);
  const lines: string[] = [];
  const journal = createJournal({
    historyPath: resolveHistory({ cwd, path: config.history }),
    writeLine: (line) => {
      lines.push(line);
    },
  });
  runTick({
    cwd,
    config,
    lanes: config.lanes,
    env: process.env,
    live: [
      {
        exclusive: [],
        wait: new Promise(() => {}),
        kill: () => {
          throw new Error("must not kill");
        },
        done: false,
        path: "tickets/agent.md",
        lane: "plan",
        runId,
      },
    ],
    journal,
  });

  const still = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(still, /^status: active$/m);
  assert.match(still, new RegExp(`^claimed-by: ${runId}$`, "m"));
  assert.match(still, /^claimed-at: /m);
  assert.equal(
    lines.some((line) => line.includes("hivemind revert")),
    false,
  );
  assert.equal(
    tsvRows(join(cwd, "hivemind.tsv")).some((row) => row[1] === "revert"),
    false,
  );
});

test("missing spawn timestamp does not revert", () => {
  const cwd = setupTtlProject({ ttl: "30m" });
  writeTicket(cwd, "agent.md", "ready-for-agent");
  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  const runId = claimedBy(claimed);
  dropSpawnRows(join(cwd, "hivemind.tsv"));

  const second = once(cwd);
  assert.equal(second.status, 0, second.stderr);
  const still = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(still, /^status: active$/m);
  assert.match(still, new RegExp(`^claimed-by: ${runId}$`, "m"));
  assert.equal(second.stderr.includes("hivemind revert"), false);
  assert.equal(
    tsvRows(join(cwd, "hivemind.tsv")).some((row) => row[1] === "revert"),
    false,
  );
});

test("pipeline ttl clock is the first spawn not a later stage", () => {
  const cwd = setupPipelineTtlProject();
  writeTicket(cwd, "agent.md", "ready-for-agent");
  const first = once(cwd);
  assert.equal(first.status, 0, first.stderr);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  const runId = claimedBy(claimed);
  const spawnRows = tsvRows(join(cwd, "hivemind.tsv")).filter(
    (row) => row[1] === "spawn" && row[4] === runId,
  );
  assert.equal(spawnRows.length, 2);
  backdateFirstSpawn(join(cwd, "hivemind.tsv"), TTL_MS + 60_000);

  const second = once(cwd);
  assert.equal(second.status, 0, second.stderr);
  const reverted = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(reverted, /^status: ready-for-agent$/m);
  assert.equal(/^claimed-by:/m.test(reverted), false);
  assert.match(second.stderr, /hivemind revert workflow tickets\/agent\.md/);
  const revertRow = tsvRows(join(cwd, "hivemind.tsv")).find(
    (row) => row[1] === "revert",
  );
  assert.equal(revertRow?.[2], "workflow");
  assert.equal(revertRow?.[4], runId);
});

function once(cwd: string): Proc {
  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "once"],
    { cwd, encoding: "utf8", timeout: 15000 },
  );
  return {
    status: proc.status,
    stdout: stdioText(proc.stdout),
    stderr: stdioText(proc.stderr),
  };
}

function stdioText(value: string | Buffer | null | undefined): string {
  if (value === null || value === undefined) return "";
  return typeof value === "string" ? value : value.toString("utf8");
}

function writeTicket(cwd: string, name: string, status: string): void {
  writeFileSync(
    join(cwd, "tickets", name),
    `---\nid: ${name.replace(/\.md$/, "")}\nstatus: ${status}\n---\n\n${BODY}`,
  );
}

function writeConfig(cwd: string, lines: string[]): void {
  mkdirSync(join(cwd, ".hivemind"), { recursive: true });
  writeFileSync(join(cwd, ".hivemind", "hivemind.yaml"), lines.join("\n"));
}

function setupTtlProject(opts: { ttl?: string }): string {
  const cwd = withTemp("hivemind-stale-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  const lane = [
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo agent-matched",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
  ];
  if (opts.ttl !== undefined) lane.push(`    ttl: ${opts.ttl}`);
  writeConfig(cwd, [
    "history: hivemind.tsv",
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "      claimed-at: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    ...lane,
    "",
  ]);
  return cwd;
}

function setupPipelineTtlProject(): string {
  const cwd = withTemp("hivemind-stale-pipe-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeConfig(cwd, [
    "history: hivemind.tsv",
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "      claimed-at: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  workflow:",
    "    type: pipeline",
    "    ttl: 30m",
    "    claim-status: active",
    "    trigger:",
    "      status: ready-for-agent",
    "    stages:",
    "      - stage: one",
    "        cmd: /bin/echo one",
    "      - stage: two",
    "        cmd: /bin/echo two",
    "",
  ]);
  return cwd;
}

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

function claimedBy(raw: string): string {
  const match = raw.match(/^claimed-by: (.+)$/m);
  assert.notEqual(match, null);
  const value = match?.[1];
  assert.equal(typeof value, "string");
  if (typeof value !== "string") throw new Error("expected claimed-by");
  return value;
}

function backdateClaimedAt(notePath: string, msAgo: number): void {
  const raw = readFileSync(notePath, "utf8");
  const match = raw.match(/^claimed-at: (.+)$/m);
  assert.notEqual(match, null);
  const value = match?.[1];
  assert.equal(typeof value, "string");
  if (typeof value !== "string") throw new Error("expected claimed-at");
  const old = Date.parse(value);
  assert.equal(Number.isNaN(old), false);
  writeFileSync(
    notePath,
    raw.replace(
      /^claimed-at: .+$/m,
      `claimed-at: ${new Date(old - msAgo).toISOString()}`,
    ),
  );
}

function backdateFirstSpawn(historyPath: string, msAgo: number): void {
  const lines = readFileSync(historyPath, "utf8").split("\n");
  let done = false;
  writeFileSync(
    historyPath,
    lines
      .map((line) => {
        const cols = line.split("\t");
        if (done || cols[1] !== "spawn" || cols[0] === undefined) return line;
        const old = Date.parse(cols[0]);
        if (Number.isNaN(old)) return line;
        cols[0] = new Date(old - msAgo).toISOString();
        done = true;
        return cols.join("\t");
      })
      .join("\n"),
  );
}

function tsvRows(historyPath: string): string[][] {
  return readFileSync(historyPath, "utf8")
    .trim()
    .split("\n")
    .slice(1)
    .map((line) => line.split("\t"));
}

function dropSpawnRows(historyPath: string): void {
  const lines = readFileSync(historyPath, "utf8").split("\n");
  writeFileSync(
    historyPath,
    lines.filter((line) => line.split("\t")[1] !== "spawn").join("\n"),
  );
}
