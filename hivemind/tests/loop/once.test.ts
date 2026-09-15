import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { loadConfig } from "../../src/config/loadConfig.ts";
import { spawnMatches } from "../../src/loop/matches.ts";
import { matchNotes } from "../../src/match/matcher.ts";
import { scan } from "../../src/scan/scan.ts";
import { withTemp } from "../with-temp.ts";

const CLI = fileURLToPath(
  new URL("../../src/cli.ts", import.meta.url),
);

type Proc = { status: number | null; stdout: string; stderr: string };

function once(cwd: string, env?: NodeJS.ProcessEnv): Proc {
  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "once"],
    { cwd, encoding: "utf8", env, timeout: 15000 },
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
    `---\nid: ${name.replace(/\.md$/, "")}\nstatus: ${status}\n---\n\n# ${name}\n`,
  );
}

function writeConfig(cwd: string, lines: string[]): void {
  mkdirSync(join(cwd, ".hivemind"), { recursive: true });
  writeFileSync(join(cwd, ".hivemind", "hivemind.yaml"), lines.join("\n"));
}

function setupMatchProject(cmd: string): string {
  const cwd = withTemp("hivemind-once-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    `    cmd: ${cmd}`,
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  return cwd;
}

test("lane trigger.status ready-for-agent ignores ready-for-human", () => {
  const cwd = setupMatchProject("/bin/echo agent-matched");
  writeTicket(cwd, "human.md", "ready-for-human");
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);

  assert.equal(proc.status, 0, proc.stderr);
  const echoed = (proc.stdout.match(/agent-matched/g) ?? []).length;
  assert.equal(echoed, 1);
  assert.equal(
    readFileSync(join(cwd, "tickets", "human.md"), "utf8").includes(
      "status: ready-for-human",
    ),
    true,
  );
});

test("two once processes cannot both take the same matching file", async () => {
  const cwd = withTemp("hivemind-cas-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "record.mjs"),
    [
      "import { writeFileSync } from 'node:fs';",
      "import { join } from 'node:path';",
      "import { randomUUID } from 'node:crypto';",
      "writeFileSync(join('spawns', randomUUID() + '.flag'), '1');",
      "",
    ].join("\n"),
  );
  mkdirSync(join(cwd, "spawns"));
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - record.mjs",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "only.md", "ready-for-agent");

  const [first, second] = await Promise.all([onceAsync(cwd), onceAsync(cwd)]);
  assert.equal(first.status, 0, first.stderr);
  assert.equal(second.status, 0, second.stderr);

  const claimed = readFileSync(join(cwd, "tickets", "only.md"), "utf8");
  assert.match(claimed, /^status: active$/m);
  assert.match(claimed, /^claimed-by: /m);
  assert.equal(claimed.match(/^status: /gm)?.length, 1);
  const flags = readdirSync(join(cwd, "spawns")).filter((name) =>
    name.endsWith(".flag"),
  );
  assert.equal(flags.length, 1);
});

test("unset {{env.SECRET}} does not spawn; interpolated metacharacters do not invoke a shell", () => {
  const cwd = setupMatchProject('/bin/echo "{{env.MISSING}}"');
  writeTicket(cwd, "agent.md", "ready-for-agent");
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    LEAK: "super-secret-value",
  };
  delete env.MISSING;
  delete env.SECRET;

  const proc = once(cwd, env);

  assert.equal(proc.status, 0, proc.stderr);
  const logs = `${proc.stdout}${proc.stderr}`;
  assert.equal(logs.includes("super-secret-value"), false);
  assert.equal((proc.stdout.match(/agent-matched/g) ?? []).length, 0);
  assert.equal(proc.stdout.includes("{{env.MISSING}}"), false);
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    /^status: ready-for-agent$/m,
  );
});

test("cmd with metacharacters in an interpolated path does not invoke a shell", () => {
  const cwd = withTemp("hivemind-meta-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    '    cmd: "/bin/echo {{prompt}}"',
    '    prompt: "foo; echo HACKED"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeFileSync(join(cwd, "foo; echo HACKED"), "x\n");
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);

  assert.equal(proc.status, 0, proc.stderr);
  assert.equal(proc.stdout.includes("foo\nHACKED"), false);
  assert.equal(proc.stdout.trim(), "foo; echo HACKED");
});

test("interpolated spaces stay one argv", () => {
  const cwd = withTemp("hivemind-spaces-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "record.mjs"),
    "import { writeFileSync } from 'node:fs';\nwriteFileSync('argv.json', JSON.stringify(process.argv.slice(2)));\n",
  );
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    `    cmd: ${process.execPath} record.mjs "{{prompt}}"`,
    '    prompt: "hello world"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeFileSync(join(cwd, "hello world"), "x\n");
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);

  assert.equal(proc.status, 0, proc.stderr);
  assert.deepEqual(JSON.parse(readFileSync(join(cwd, "argv.json"), "utf8")), [
    "hello world",
  ]);
});

test("overlapping live exclusive/scope skip", async () => {
  const cwd = withTemp("hivemind-excl-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  mkdirSync(join(cwd, "spawns"));
  writeFileSync(
    join(cwd, "record.mjs"),
    [
      "import { writeFileSync } from 'node:fs';",
      "import { join } from 'node:path';",
      "import { randomUUID } from 'node:crypto';",
      "writeFileSync(join('spawns', randomUUID() + '.flag'), String(process.pid));",
      "await new Promise((resolve) => setTimeout(resolve, 400));",
      "",
    ].join("\n"),
  );
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    concurrency: 2",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - record.mjs",
    "    trigger:",
    "      status: ready-for-agent",
    "    exclusive:",
    "      - tickets",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "one.md", "ready-for-agent");
  writeTicket(cwd, "two.md", "ready-for-agent");

  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  const flags = readdirSync(join(cwd, "spawns")).filter((name) =>
    name.endsWith(".flag"),
  );
  assert.equal(flags.length, 1);
  const claimed = ["one.md", "two.md"].filter((name) =>
    readFileSync(join(cwd, "tickets", name), "utf8").includes("status: active"),
  );
  assert.equal(claimed.length, 1);
});

test("child is one unit; supervisor does not loop tickets inside the child", () => {
  const cwd = withTemp("hivemind-unit-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  mkdirSync(join(cwd, "spawns"));
  writeFileSync(
    join(cwd, "record.mjs"),
    [
      "import { writeFileSync } from 'node:fs';",
      "import { join } from 'node:path';",
      "writeFileSync(join('spawns', process.pid + '.flag'), process.argv.join('\\0'));",
      "",
    ].join("\n"),
  );
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - record.mjs",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "one.md", "ready-for-agent");
  writeTicket(cwd, "two.md", "ready-for-agent");

  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  const flags = readdirSync(join(cwd, "spawns")).filter((name) =>
    name.endsWith(".flag"),
  );
  assert.equal(flags.length, 1);
  const flag = flags[0];
  if (flag === undefined) throw new Error("expected a spawn flag");
  const payload = readFileSync(join(cwd, "spawns", flag), "utf8");
  const both = payload.includes("one.md") && payload.includes("two.md");
  assert.equal(both, false);
  const statuses = ["one.md", "two.md"].map((name) =>
    readFileSync(join(cwd, "tickets", name), "utf8"),
  );
  const active = statuses.filter((text) => /^status: active$/m.test(text));
  const ready = statuses.filter((text) =>
    /^status: ready-for-agent$/m.test(text),
  );
  assert.equal(active.length, 1);
  assert.equal(ready.length, 1);
});

test("dest cmd containing {{run-id}} receives the minted id", () => {
  const cwd = withTemp("hivemind-run-id-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "record.mjs"),
    "import { writeFileSync } from 'node:fs';\nwriteFileSync('argv.json', JSON.stringify(process.argv.slice(2)));\n",
  );
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - record.mjs",
    '      - "{{run-id}}"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);

  assert.equal(proc.status, 0, proc.stderr);
  const argv: unknown = JSON.parse(
    readFileSync(join(cwd, "argv.json"), "utf8"),
  );
  assert.equal(Array.isArray(argv), true);
  if (!Array.isArray(argv)) return;
  assert.equal(argv.length, 1);
  const runId = argv[0];
  assert.equal(typeof runId, "string");
  if (typeof runId !== "string") return;
  assert.match(
    runId,
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    new RegExp(`^claimed-by: ${runId}$`, "m"),
  );
});

test("missing prompt file does not spawn and does not claim", () => {
  const cwd = withTemp("hivemind-prompt-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo spawned",
    "    prompt: missing.md",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  assert.equal(proc.stdout.includes("spawned"), false);
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    /^status: ready-for-agent$/m,
  );
});

test("live claimed-by skip does not re-spawn the same path", () => {
  const cwd = setupMatchProject("/bin/echo spawned");
  writeTicket(cwd, "agent.md", "ready-for-agent");
  const config = loadConfig(cwd);
  const { notes } = scan({ cwd, config });
  const matches = matchNotes({ lanes: config.lanes, notes });
  const spawned: unknown[] = [];
  const n = spawnMatches({
    cwd,
    matches,
    env: process.env,
    spawnChild: (opts) => {
      spawned.push(opts.argv);
      return { wait: Promise.resolve(0), kill: () => {} };
    },
    live: [
      {
        exclusive: [],
        wait: new Promise(() => {}),
        kill: () => {},
        done: false,
        path: "tickets/agent.md",
        lane: "plan",
        runId: "live-run",
      },
    ],
  });
  assert.equal(n, 0);
  assert.deepEqual(spawned, []);
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    /^status: ready-for-agent$/m,
  );
});

test("once prints scan claim spawn exit to stderr", () => {
  const cwd = setupMatchProject("/bin/echo agent-matched");
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);

  assert.equal(proc.status, 0, proc.stderr);
  assert.match(proc.stderr, /hivemind scan notes=1 quarantined=0/);
  assert.match(proc.stderr, /hivemind claim plan tickets\/agent\.md/);
  assert.match(proc.stderr, /hivemind spawn plan tickets\/agent\.md/);
  assert.match(proc.stderr, /hivemind exit plan tickets\/agent\.md status=0/);
  assert.match(proc.stdout, /agent-matched/);
  assert.equal(existsSync(join(cwd, "hivemind.tsv")), false);
});

test("once appends those actions to the history TSV", () => {
  const cwd = withTemp("hivemind-history-");
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
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo agent-matched",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);

  assert.equal(proc.status, 0, proc.stderr);
  const rows = readFileSync(join(cwd, "hivemind.tsv"), "utf8")
    .trim()
    .split("\n");
  assert.equal(rows[0], "ts\taction\tlane\tpath\trun_id\tdetail");
  const actions = rows.slice(1).map((row) => row.split("\t")[1]);
  assert.deepEqual(actions, ["scan", "claim", "spawn", "exit"]);
  assert.match(rows[1] ?? "", /notes=1 quarantined=0/);
  assert.match(rows[2] ?? "", /\tplan\ttickets\/agent\.md\t/);
});

test("once logs quarantine of a faulty note", () => {
  const cwd = withTemp("hivemind-history-q-");
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
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo spawned",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeFileSync(join(cwd, "tickets", "bad.md"), "---\n{\n---\n");

  const proc = once(cwd);

  assert.equal(proc.status, 0, proc.stderr);
  assert.match(proc.stderr, /hivemind quarantine tickets\/bad\.md parse-error/);
  assert.match(proc.stderr, /hivemind scan notes=0 quarantined=1/);
  assert.equal(proc.stdout.includes("spawned"), false);
  const text = readFileSync(join(cwd, "hivemind.tsv"), "utf8");
  assert.match(text, /\tquarantine\t\ttickets\/bad\.md\t\tparse-error/);
});

test("skip for unset env is logged without the secret value", () => {
  const cwd = setupMatchProject('/bin/echo "{{env.MISSING}}"');
  writeTicket(cwd, "agent.md", "ready-for-agent");
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    LEAK: "super-secret-value",
  };
  delete env.MISSING;

  const proc = once(cwd, env);

  assert.equal(proc.status, 0, proc.stderr);
  assert.match(proc.stderr, /hivemind skip plan tickets\/agent\.md cmd-skip/);
  assert.equal(proc.stderr.includes("super-secret-value"), false);
  assert.equal(proc.stdout.includes("super-secret-value"), false);
});

test("independent lanes do not share a concurrency pool", () => {
  const cwd = withTemp("hivemind-lanes-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  mkdirSync(join(cwd, "spawns"));
  writeFileSync(
    join(cwd, "record.mjs"),
    [
      "import { writeFileSync } from 'node:fs';",
      "import { join } from 'node:path';",
      "writeFileSync(join('spawns', process.argv[2] + '.flag'), '1');",
      "",
    ].join("\n"),
  );
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    concurrency: 1",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - record.mjs",
    "      - plan",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "  build:",
    "    type: single",
    "    concurrency: 1",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - record.mjs",
    "      - build",
    "    trigger:",
    "      status: ready-for-build",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "plan.md", "ready-for-agent");
  writeTicket(cwd, "build.md", "ready-for-build");

  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  const flags = readdirSync(join(cwd, "spawns")).filter((name) =>
    name.endsWith(".flag"),
  );
  assert.deepEqual(flags.sort(), ["build.flag", "plan.flag"]);
});

test("hivemind.spawn:cmd-only-single: once claims and execs argv with no shell", () => {
  const cwd = withTemp("hivemind-cmd-only-single-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "record.mjs"),
    "import { writeFileSync } from 'node:fs';\nwriteFileSync('argv.json', JSON.stringify(process.argv.slice(2)));\n",
  );
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - record.mjs",
    '      - "foo; echo HACKED"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  assert.equal(proc.stdout.includes("HACKED"), false);
  assert.deepEqual(JSON.parse(readFileSync(join(cwd, "argv.json"), "utf8")), [
    "foo; echo HACKED",
  ]);
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(claimed, /^status: active$/m);
  assert.match(claimed, /^claimed-by: /m);
  assert.equal(claimed.match(/^status: /gm)?.length, 1);
  assert.equal(claimed.match(/^claimed-by: /gm)?.length, 1);
});

test("pipeline runs stages in order and stops on failure", () => {
  const cwd = withTemp("hivemind-pipe-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  mkdirSync(join(cwd, "spawns"));
  writeFileSync(
    join(cwd, "stage.mjs"),
    [
      "import { appendFileSync } from 'node:fs';",
      "appendFileSync('spawns/order.log', process.argv[2] + '\\n');",
      "process.exit(Number(process.argv[3]));",
      "",
    ].join("\n"),
  );
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "lanes:",
    "  workflow:",
    "    type: pipeline",
    "    concurrency: 1",
    "    claim-status: active",
    "    trigger:",
    "      status: ready-for-agent",
    "    stages:",
    "      - stage: one",
    "        cmd:",
    `          - ${process.execPath}`,
    "          - stage.mjs",
    "          - one",
    '          - "0"',
    "      - stage: two",
    "        cmd:",
    `          - ${process.execPath}`,
    "          - stage.mjs",
    "          - two",
    '          - "1"',
    "      - stage: three",
    "        cmd:",
    `          - ${process.execPath}`,
    "          - stage.mjs",
    "          - three",
    '          - "0"',
    "",
  ]);
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  assert.equal(
    readFileSync(join(cwd, "spawns", "order.log"), "utf8"),
    "one\ntwo\n",
  );
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    /^status: active$/m,
  );
});

test("hivemind.loop:cmd-mix-pipeline: cmd-only stages run despite absent actor prompt; order; stop on non-zero; one claim", () => {
  const cwd = withTemp("hivemind-cmd-mix-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  mkdirSync(join(cwd, "spawns"));
  mkdirSync(join(cwd, "prompts"));
  writeFileSync(join(cwd, "prompts", "agent.md"), "# agent\n");
  writeFileSync(
    join(cwd, "stage.mjs"),
    [
      "import { appendFileSync } from 'node:fs';",
      "appendFileSync('spawns/order.log', process.argv[2] + '\\n');",
      "process.exit(Number(process.argv.at(-1)));",
      "",
    ].join("\n"),
  );
  writeConfig(cwd, [
    "history: hivemind.tsv",
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "actors:",
    "  pi:",
    "    cmd:",
    `      - ${process.execPath}`,
    "      - stage.mjs",
    "      - agent",
    '      - "{{prompt}}"',
    '      - "1"',
    "    agent: heio-builder",
    "    prompt: missing-actor.md",
    "    claim-status: active",
    "lanes:",
    "  workflow:",
    "    type: pipeline",
    "    actor: pi",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "    stages:",
    "      - stage: lint",
    "        cmd:",
    `          - ${process.execPath}`,
    "          - stage.mjs",
    "          - lint",
    '          - "0"',
    "      - stage: build",
    "        agent: heio-builder",
    "        prompt: prompts/agent.md",
    "      - stage: check",
    "        cmd:",
    `          - ${process.execPath}`,
    "          - stage.mjs",
    "          - check",
    '          - "0"',
    "",
  ]);
  writeTicket(cwd, "agent.md", "ready-for-agent");

  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  assert.equal(proc.stderr.includes("missing-prompt"), false);
  assert.equal(
    readFileSync(join(cwd, "spawns", "order.log"), "utf8"),
    "lint\nagent\n",
  );
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(claimed, /^status: active$/m);
  assert.match(claimed, /^claimed-by: /m);
  assert.equal(claimed.match(/^status: /gm)?.length, 1);
  assert.equal(claimed.match(/^claimed-by: /gm)?.length, 1);
  const rows = readFileSync(join(cwd, "hivemind.tsv"), "utf8")
    .trim()
    .split("\n");
  const events = rows.slice(1).map((row) => {
    const cols = row.split("\t");
    return { action: cols[1] ?? "", detail: cols[5] ?? "" };
  });
  assert.deepEqual(
    events.map((event) => event.action),
    ["scan", "claim", "spawn", "exit", "spawn", "exit"],
  );
  assert.match(events[2]?.detail ?? "", /(?:^| )stage=lint(?: |$)/);
  assert.match(events[3]?.detail ?? "", /status=0 stage=lint(?: |$)/);
  assert.match(events[4]?.detail ?? "", /(?:^| )stage=build(?: |$)/);
  assert.match(events[5]?.detail ?? "", /status=1 stage=build(?: |$)/);
  assert.equal(
    events.some((event) => /stage=check/.test(event.detail)),
    false,
  );
});

test("once with no matching files exits zero and spawns nothing", () => {
  const cwd = setupMatchProject("/bin/echo should-not-run");
  writeTicket(cwd, "human.md", "ready-for-human");
  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  assert.equal((proc.stdout.match(/should-not-run/g) ?? []).length, 0);
});

test("disable omits those lane ids from once", () => {
  const cwd = setupMatchProject("/bin/echo should-not-run");
  writeConfig(cwd, [
    "folders:",
    "  - path: tickets",
    "    schema:",
    "      id: string",
    "      status: string",
    "      claimed-by: string",
    "    required: [id, status]",
    "  - path: quarantine",
    "    schema: quarantine",
    "    required: [origin-location, quarantined-at, fault]",
    "disable: [plan]",
    "lanes:",
    "  plan:",
    "    type: single",
    "    cmd: /bin/echo should-not-run",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeTicket(cwd, "agent.md", "ready-for-agent");
  const proc = once(cwd);
  assert.equal(proc.status, 0, proc.stderr);
  assert.equal((proc.stdout.match(/should-not-run/g) ?? []).length, 0);
  assert.match(
    readFileSync(join(cwd, "tickets", "agent.md"), "utf8"),
    /^status: ready-for-agent$/m,
  );
});

function onceAsync(cwd: string): Promise<Proc> {
  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      ["--experimental-strip-types", CLI, "once"],
      {
        cwd,
      },
    );
    let stdout = "";
    let stderr = "";
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      child.kill("SIGKILL");
      reject(new Error("onceAsync timed out after 15000ms"));
    }, 15000);
    child.stdout.on("data", (chunk: Buffer | string) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk: Buffer | string) => {
      stderr += chunk.toString();
    });
    child.on("error", (err) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      reject(err);
    });
    child.on("close", (status) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ status, stdout, stderr });
    });
  });
}
