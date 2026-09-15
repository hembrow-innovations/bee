import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { withTemp } from "./with-temp.ts";
import { run } from "../src/cli.ts";

function writeConfig(cwd: string, body: string): void {
  mkdirSync(join(cwd, ".hivemind"), { recursive: true });
  writeFileSync(join(cwd, ".hivemind", "hivemind.yaml"), body);
}

const CLI = fileURLToPath(
  new URL("../src/cli.ts", import.meta.url),
);

test("once with no .hivemind/hivemind.yaml exits non-zero, no child", async () => {
  const cwd = withTemp("hivemind-missing-");
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["once"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.notEqual(status, 0);
  assert.deepEqual(spawned, []);

  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "once"],
    { cwd, encoding: "utf8", timeout: 15000 },
  );
  assert.notEqual(proc.status, 0);
  assert.equal(proc.status, status);
});

test("unknown keys exit non-zero", async () => {
  const cwd = withTemp("hivemind-unknown-");
  writeConfig(cwd, "folders: []\nlanes: {}\nunknown: 1\n");
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["once"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.notEqual(status, 0);
  assert.deepEqual(spawned, []);
});

test("empty lanes: {} exits zero and spawns nothing", async () => {
  const cwd = withTemp("hivemind-empty-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["once"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(status, 0);
  assert.deepEqual(spawned, []);

  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "once"],
    { cwd, encoding: "utf8", timeout: 15000 },
  );
  assert.equal(proc.status, 0);
});

test("help exits zero", async () => {
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["--help"],
    cwd: withTemp("hivemind-help-"),
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(status, 0);
  assert.deepEqual(spawned, []);
});

test("help lists watch flags", () => {
  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "--help"],
    { encoding: "utf8", timeout: 15000 },
  );
  assert.equal(proc.status, 0);
  assert.match(proc.stdout, /src\/cli\.ts/);
  assert.match(proc.stdout, /--until-quiet/);
  assert.match(proc.stdout, /--until-target/);
  assert.match(proc.stdout, /--max-spawns/);
  assert.match(proc.stdout, /\bstatus\b/);
  assert.match(proc.stdout, /explain <path>/);
  assert.match(proc.stdout, /--dry-run/);
  assert.match(proc.stdout, /\bgc\b/);
});

test("unknown command exits non-zero, no child", async () => {
  const cwd = withTemp("hivemind-unknown-cmd-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["mint"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.notEqual(status, 0);
  assert.deepEqual(spawned, []);
});

test("unknown watch flag exits non-zero", async () => {
  const cwd = withTemp("hivemind-unknown-flag-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["watch", "--forever"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.notEqual(status, 0);
  assert.deepEqual(spawned, []);
});

test("hivemind.cli:max-spawns without a count exits non-zero", async () => {
  const cwd = withTemp("hivemind-max-spawns-missing-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["watch", "--max-spawns"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.notEqual(status, 0);
  assert.deepEqual(spawned, []);
});

test("--until-target without a path exits non-zero", async () => {
  const cwd = withTemp("hivemind-until-target-missing-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["watch", "--until-target"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.notEqual(status, 0);
  assert.deepEqual(spawned, []);
});

test("hivemind.config:claim-status-eq-trigger: claim-status equal to trigger.status exits non-zero, no child", async () => {
  const cwd = withTemp("hivemind-claim-eq-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "lanes:",
      "  build:",
      "    type: single",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: active",
      "    claim-status: active",
      "",
    ].join("\n"),
  );
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["once"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.notEqual(status, 0);
  assert.deepEqual(spawned, []);
});

test("hivemind.config:lane-ttl-illegal: illegal ttl or positive ttl without history exits non-zero, no child", async () => {
  const cases = ["30", '"30"', "1d", "30M", "true", '""', "30m"];
  for (const value of cases) {
    const cwd = withTemp("hivemind-ttl-illegal-");
    writeConfig(
      cwd,
      [
        "folders: []",
        "lanes:",
        "  plan:",
        "    type: single",
        "    cmd: /bin/echo",
        "    trigger:",
        "      status: ready",
        "    claim-status: active",
        `    ttl: ${value}`,
        "",
      ].join("\n"),
    );
    const spawned: unknown[] = [];
    const status = await run({
      argv: ["once"],
      cwd,
      spawnChild: (opts) => {
        spawned.push(opts.argv);
        return { wait: Promise.resolve(0), kill: () => {} };
      },
    });
    assert.notEqual(status, 0, `ttl: ${value}`);
    assert.deepEqual(spawned, [], `ttl: ${value}`);
  }
});

function setupOperatorProject(lanes: string[]): string {
  const cwd = withTemp("hivemind-operator-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  mkdirSync(join(cwd, "logs"));
  writeConfig(
    cwd,
    [
      "folders:",
      "  - path: tickets",
      "    schema:",
      "      id: string",
      "      status: string",
      "      claimed-by: string",
      "      sealed: bool",
      "    required: [id, status]",
      "  - path: quarantine",
      "    schema: quarantine",
      "    required: [origin-location, quarantined-at, fault]",
      "history: logs/hivemind.tsv",
      "lanes:",
      ...lanes,
      "",
    ].join("\n"),
  );
  return cwd;
}

function writeTicket(cwd: string, name: string, yaml: string): void {
  writeFileSync(join(cwd, "tickets", name), `---\n${yaml}\n---\n\n# ${name}\n`);
}

test("hivemind.cli:status is read-only and lists live runs", async () => {
  const cwd = setupOperatorProject([
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo should-not-run",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
  ]);
  writeTicket(cwd, "ready.md", "id: ready\nstatus: ready-for-agent");
  const historyPath = join(cwd, "logs", "hivemind.tsv");
  const history = [
    "ts\taction\tlane\tpath\trun_id\tdetail",
    "2026-09-05T20:00:00.000Z\tscan\t\t\t\tnotes=2 quarantined=1",
    "2026-09-05T20:00:01.000Z\tquarantine\t\ttickets/bad.md\t\tparse-error",
    "2026-09-05T20:00:02.000Z\tskip\tplan\ttickets/busy.md\t\tconcurrency",
    "2026-09-05T20:00:03.000Z\tskip\tplan\ttickets/busy.md\t\tconcurrency",
    "2026-09-05T20:00:04.000Z\tskip\tbuild\ttickets/held.md\t\texclusive",
    "2026-09-05T20:00:05.000Z\tspawn\tplan\ttickets/live.md\trun-live\tpid=8122",
    "2026-09-05T20:00:06.000Z\trevert\tplan\ttickets/stale.md\trun-stale\t",
    "2026-09-05T20:00:07.000Z\tspawn\tbuild\ttickets/done.md\trun-done\tpid=9001",
    "2026-09-05T20:00:08.000Z\texit\tbuild\ttickets/done.md\trun-done\tstatus=0 pid=9001",
    "",
  ].join("\n");
  writeFileSync(historyPath, history);
  const ticketBefore = readFileSync(join(cwd, "tickets", "ready.md"), "utf8");

  const spawned: unknown[] = [];
  const status = await run({
    argv: ["status"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(status, 0);
  assert.deepEqual(spawned, []);
  assert.equal(
    readFileSync(join(cwd, "tickets", "ready.md"), "utf8"),
    ticketBefore,
  );
  assert.equal(readFileSync(historyPath, "utf8"), history);

  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "status"],
    { cwd, encoding: "utf8", timeout: 15000 },
  );
  assert.equal(proc.status, 0, proc.stderr);
  assert.match(proc.stdout, /last-scan notes=2 quarantined=1/);
  assert.match(
    proc.stdout,
    /live lane=plan path=tickets\/live\.md pid=8122 run-id=run-live age=\d+s/,
  );
  assert.equal(proc.stdout.includes("tickets/done.md"), false);
  assert.match(proc.stdout, /skip concurrency=2 exclusive=1/);
  assert.match(proc.stdout, /quarantined 1/);
  assert.match(proc.stdout, /ttl-expired 1/);
  assert.equal(proc.stdout.includes("should-not-run"), false);
  assert.equal(
    readFileSync(join(cwd, "tickets", "ready.md"), "utf8"),
    ticketBefore,
  );
  assert.equal(readFileSync(historyPath, "utf8"), history);
});

test("hivemind.cli:explain names match, need miss, or exclusive skip", async () => {
  const cwd = setupOperatorProject([
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo should-not-run",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "  build:",
    "    type: single",
    "    cmd: /bin/echo should-not-run",
    "    trigger:",
    "      status: ready-for-agent",
    "    need:",
    "      sealed: true",
    "    claim-status: active",
    "  review:",
    "    type: single",
    "    cmd: /bin/echo should-not-run",
    "    exclusive: [tickets]",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
  ]);
  writeTicket(cwd, "agent.md", "id: agent\nstatus: ready-for-agent");
  writeTicket(cwd, "other.md", "id: other\nstatus: ready-for-agent");
  const historyPath = join(cwd, "logs", "hivemind.tsv");
  const history = [
    "ts\taction\tlane\tpath\trun_id\tdetail",
    "2026-09-05T20:00:00.000Z\tskip\treview\ttickets/agent.md\t\texclusive",
    "2026-09-05T20:00:01.000Z\tspawn\treview\ttickets/other.md\trun-live\tpid=8122",
    "",
  ].join("\n");
  writeFileSync(historyPath, history);
  const agentBefore = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  const otherBefore = readFileSync(join(cwd, "tickets", "other.md"), "utf8");

  const spawned: unknown[] = [];
  const status = await run({
    argv: ["explain", "tickets/agent.md"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(status, 0);
  assert.deepEqual(spawned, []);
  assert.equal(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    agentBefore,
  );
  assert.equal(
    readFileSync(join(cwd, "tickets", "other.md"), "utf8"),
    otherBefore,
  );
  assert.equal(readFileSync(historyPath, "utf8"), history);

  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "explain", "tickets/agent.md"],
    { cwd, encoding: "utf8", timeout: 15000 },
  );
  assert.equal(proc.status, 0, proc.stderr);
  assert.match(proc.stdout, /match plan/);
  assert.match(proc.stdout, /need build/);
  assert.match(proc.stdout, /exclusive review/);
  assert.match(proc.stdout, /last-skip exclusive/);
  assert.equal(proc.stdout.includes("should-not-run"), false);
  assert.equal(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    agentBefore,
  );
  assert.equal(
    readFileSync(join(cwd, "tickets", "other.md"), "utf8"),
    otherBefore,
  );
});

test("hivemind.cli:once-dry-run prints the plan, exits 0, no claim no spawn", async () => {
  const cwd = setupOperatorProject([
    "  plan:",
    "    type: single",
    '    cmd: "/bin/echo {{env.SECRET}}"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
  ]);
  writeTicket(cwd, "agent.md", "id: agent\nstatus: ready-for-agent");
  const ticketBefore = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  const historyPath = join(cwd, "logs", "hivemind.tsv");
  const env = {
    ...process.env,
    SECRET: "super-secret-value",
  };

  const spawned: unknown[] = [];
  const status = await run({
    argv: ["once", "--dry-run"],
    cwd,
    env,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(status, 0);
  assert.deepEqual(spawned, []);
  assert.equal(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    ticketBefore,
  );
  assert.equal(existsSync(historyPath), false);

  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "once", "--dry-run"],
    { cwd, encoding: "utf8", env, timeout: 15000 },
  );
  assert.equal(proc.status, 0, proc.stderr);
  assert.match(
    proc.stdout,
    /plan lane=plan path=tickets\/agent\.md argv=\/bin\/echo \*\*\*/,
  );
  assert.equal(proc.stdout.includes("super-secret-value"), false);
  assert.equal(proc.stderr.includes("super-secret-value"), false);
  assert.equal(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    ticketBefore,
  );
  assert.equal(existsSync(historyPath), false);
});

test("hivemind.journal:gc: hivemind gc archives old history", async () => {
  const cwd = setupOperatorProject([
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo should-not-run",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
  ]);
  const historyPath = join(cwd, "logs", "hivemind.tsv");
  const oldTs = new Date(Date.now() - 40 * 86400000).toISOString();
  const liveTs = new Date(Date.now() - 2 * 86400000).toISOString();
  writeFileSync(
    historyPath,
    [
      "ts\taction\tlane\tpath\trun_id\tdetail",
      `${oldTs}\tspawn\tplan\ttickets/old.md\trun-old\t`,
      `${liveTs}\tspawn\tplan\ttickets/live.md\trun-live\t`,
      "",
    ].join("\n"),
  );
  const spawned: unknown[] = [];
  const status = await run({
    argv: ["gc", "--days", "30"],
    cwd,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
  });
  assert.equal(status, 0);
  assert.deepEqual(spawned, []);
  const live = readFileSync(historyPath, "utf8");
  assert.equal(live.includes("run-old"), false);
  assert.match(live, /run-live/);
  assert.match(readFileSync(`${historyPath}.archive`, "utf8"), /run-old/);
});
