import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { withTemp } from "../with-temp.ts";
import {
  loadConfig,
  loadConfigFile,
} from "../../src/config/loadConfig.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const WORKBENCH = join(HERE, "../../..");
const FIXTURE = join(WORKBENCH, "tests/fixtures/heio-stack.hivemind.yaml");
const DEST_YAML = join(WORKBENCH, ".hivemind/hivemind.yaml");
const OPENCODE_CMD = [
  "opencode",
  "run",
  "--auto",
  "--format",
  "json",
  "--agent",
  "{{agent}}",
  "--file",
  "{{prompt}}",
];

function writeConfig(cwd: string, body: string): void {
  mkdirSync(join(cwd, ".hivemind"), { recursive: true });
  writeFileSync(join(cwd, ".hivemind", "hivemind.yaml"), body);
}

test("loadConfig throws when .hivemind/hivemind.yaml is missing", () => {
  const cwd = withTemp("hivemind-load-missing-");
  assert.throws(() => loadConfig(cwd), /Missing \.hivemind\/hivemind\.yaml/);
});

test("loadConfig ignores a root hivemind.yaml", () => {
  const cwd = withTemp("hivemind-load-root-");
  writeFileSync(join(cwd, "hivemind.yaml"), "folders: []\nlanes: {}\n");
  assert.throws(() => loadConfig(cwd), /Missing \.hivemind\/hivemind\.yaml/);
});

test("loadConfig throws on unknown top-level keys", () => {
  const cwd = withTemp("hivemind-load-unknown-");
  writeConfig(cwd, "folders: []\nlanes: {}\nunknown: 1\n");
  assert.throws(() => loadConfig(cwd), /Unknown key "unknown"/);
});

test("notes: does not fail dest load", () => {
  const cwd = withTemp("hivemind-load-notes-");
  writeConfig(
    cwd,
    "folders: []\nlanes: {}\nnotes:\n  planning: .heio/planning\n",
  );
  const cfg = loadConfig(cwd);
  assert.deepEqual(cfg.lanes, []);
});

test("loadConfig rejects top-level concurrency", () => {
  const cwd = withTemp("hivemind-load-global-conc-");
  writeConfig(cwd, "concurrency: 2\nfolders: []\nlanes: {}\n");
  assert.throws(() => loadConfig(cwd), /Unknown key "concurrency"/);
});

test("loadConfig accepts empty lanes: {}", () => {
  const cwd = withTemp("hivemind-load-empty-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  const cfg = loadConfig(cwd);
  assert.deepEqual(cfg.lanes, []);
});

test("loadConfig rejects lanes as a list", () => {
  const cwd = withTemp("hivemind-load-list-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "lanes:",
      "  - lane: plan",
      "    type: single",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: ready",
      "    claim-status: active",
      "",
    ].join("\n"),
  );
  assert.throws(() => loadConfig(cwd), /"lanes" must be a map/);
});

test("loadConfig reads map lanes and per-lane concurrency", () => {
  const cwd = withTemp("hivemind-load-map-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "lanes:",
      "  plan:",
      "    type: single",
      "    concurrency: 2",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: ready-for-agent",
      "    claim-status: active",
      "  build:",
      "    type: single",
      "    cmd: /bin/true",
      "    trigger:",
      "      status: ready",
      "    claim-status: active",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  assert.deepEqual(
    cfg.lanes.map((lane) => [lane.lane, lane.type, lane.concurrency]),
    [
      ["plan", "single", 2],
      ["build", "single", 1],
    ],
  );
});

test("loadConfig merges actors from files and overlays hivemind.yaml", () => {
  const cwd = withTemp("hivemind-load-actors-");
  mkdirSync(join(cwd, ".hivemind", "actors"), { recursive: true });
  writeFileSync(
    join(cwd, ".hivemind", "actors", "planner.yaml"),
    [
      "cmd: /bin/echo file",
      "agent: from-file",
      "claim-status: active",
      "",
    ].join("\n"),
  );
  writeFileSync(
    join(cwd, ".hivemind", "actors", "roles.yaml"),
    ["builder:", "  cmd: /bin/echo builder", "  claim-status: active", ""].join(
      "\n",
    ),
  );
  writeConfig(
    cwd,
    [
      "folders: []",
      "actors:",
      "  planner:",
      "    cmd: /bin/echo yaml",
      "    agent: from-yaml",
      "    claim-status: active",
      "lanes:",
      "  plan:",
      "    type: single",
      "    actor: planner",
      "    trigger:",
      "      status: ready",
      "  build:",
      "    type: single",
      "    actor: builder",
      "    trigger:",
      "      status: ready",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  const plan = cfg.lanes.find((lane) => lane.lane === "plan");
  const build = cfg.lanes.find((lane) => lane.lane === "build");
  assert.equal(plan?.type, "single");
  if (plan?.type !== "single" || build?.type !== "single") {
    throw new Error("expected single lanes");
  }
  assert.equal(plan.cmd, "/bin/echo yaml");
  assert.equal(plan.agent, "from-yaml");
  assert.equal(build.cmd, "/bin/echo builder");
});

test("loadConfig fails on duplicate actor names across files", () => {
  const cwd = withTemp("hivemind-load-dup-actor-");
  mkdirSync(join(cwd, ".hivemind", "actors"), { recursive: true });
  writeFileSync(
    join(cwd, ".hivemind", "actors", "a.yaml"),
    "cmd: /bin/echo a\nclaim-status: active\n",
  );
  writeFileSync(
    join(cwd, ".hivemind", "actors", "a.yml"),
    "cmd: /bin/echo b\nclaim-status: active\n",
  );
  writeConfig(
    cwd,
    [
      "folders: []",
      "lanes:",
      "  plan:",
      "    type: single",
      "    actor: a",
      "    trigger:",
      "      status: ready",
      "",
    ].join("\n"),
  );
  assert.throws(() => loadConfig(cwd), /Duplicate actor "a"/);
});

test("loadConfig parses a pipeline lane", () => {
  const cwd = withTemp("hivemind-load-pipe-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "lanes:",
      "  workflow:",
      "    type: pipeline",
      "    concurrency: 1",
      "    cooldown: 90s",
      "    claim-status: active",
      "    trigger:",
      "      status: ready-for-agent",
      "    stages:",
      "      - stage: plan",
      "        cmd: /bin/echo plan",
      "      - stage: build",
      "        cmd: /bin/echo build",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  const lane = cfg.lanes[0];
  assert.equal(lane?.type, "pipeline");
  if (lane?.type !== "pipeline") throw new Error("expected pipeline");
  assert.equal(lane.concurrency, 1);
  assert.equal(lane.cooldownMs, 90_000);
  assert.deepEqual(
    lane.stages.map((stage) => stage.stage),
    ["plan", "build"],
  );
});

test("heio-stack hivemind.yaml names .heio/ folders, quarantine, sealed/ready, Plan/Build/Review; Mint omitted or disable; no Tasker", () => {
  const text = readFileSync(FIXTURE, "utf8");
  assert.match(text, /\.heio\/tickets/);
  assert.match(text, /\.heio\/quarantine/);
  assert.match(text, /schema: quarantine/);
  assert.equal(/schema: ticket/.test(text), false);
  assert.equal(/schema: planning/.test(text), false);
  assert.equal(/schema: archive/.test(text), false);
  const archive = text.slice(text.indexOf("path: .heio/archive"));
  const archiveBlock = archive.slice(
    0,
    archive.indexOf("path: .heio/quarantine"),
  );
  for (const key of [
    "id",
    "title",
    "kind",
    "status",
    "labels",
    "tags",
    "sprint",
    "created_at",
    "updated_at",
    "blocked-by",
    "mode",
    "sitting-kind",
  ]) {
    assert.match(archiveBlock, new RegExp(`^\\s+${key}: string$`, "m"));
  }
  assert.match(text, /sealed/);
  assert.match(text, /ready/);
  assert.match(text, /^ {2}plan:/m);
  assert.doesNotMatch(text, /^ {2}tasker:/m);
  assert.doesNotMatch(text, /heio-tasker/);
  assert.match(text, /^ {2}build:/m);
  assert.match(text, /^ {2}review:/m);
  assert.equal(/\n {2}mint:/m.test(text), false);
  assert.doesNotMatch(text, /^disable:\s*\[\s*\]/m);
  assert.doesNotMatch(text, /^concurrency:/m);
});

test("loadConfigFile accepts the heio-stack template", () => {
  const cfg = loadConfigFile({
    file: FIXTURE,
  });
  assert.deepEqual(
    cfg.lanes.map((lane) => lane.lane),
    ["plan", "build", "review"],
  );
  for (const lane of cfg.lanes) {
    assert.equal(lane.type, "single");
    assert.equal(typeof lane.claimStatus, "string");
    assert.notEqual(lane.claimStatus, "");
    assert.equal(typeof lane.concurrency, "number");
  }
  const review = cfg.lanes.find((lane) => lane.lane === "review");
  assert.equal(review?.type, "single");
  if (review?.type !== "single") throw new Error("expected single");
  assert.equal(review.scalars["mint-status"], "ready-for-human");
  const mintLane = cfg.lanes.some((lane) => lane.lane === "mint");
  assert.equal(mintLane && !cfg.disable.includes("mint"), false);
  assert.equal(cfg.watch, undefined);
  assert.equal(cfg.history, ".heio/logs/hivemind.tsv");
});

test("heio-stack fixture is the pack template, not this dest's Pi agent", () => {
  const pack = readFileSync(FIXTURE, "utf8");
  const dest = readFileSync(DEST_YAML, "utf8");
  assert.notEqual(pack, dest);
  assert.match(dest, /^ {2}drain:/m);
  assert.match(dest, /heio-drain/);
  assert.doesNotMatch(dest, /^ {2}tasker:/m);
  assert.doesNotMatch(pack, /^ {2}drain:/m);
  assert.doesNotMatch(pack, /^ {2}tasker:/m);
});

test("hivemind.template:review-actor-gates", () => {
  const text = readFileSync(FIXTURE, "utf8");
  const cfg = loadConfigFile({ file: FIXTURE });
  assert.deepEqual(
    cfg.lanes.map((lane) => lane.lane),
    ["plan", "build", "review"],
  );
  const examples = text
    .split("\n")
    .filter((line) => /pi --model|codex exec/.test(line));
  assert.ok(examples.length > 0, "commented second review actor example");
  for (const line of examples) {
    assert.match(line, /^\s*#/);
  }
  assert.doesNotMatch(text, /^ {2}pi:/m);
  assert.doesNotMatch(text, /^ {2}codex:/m);
  assert.match(text, /EMPTY MATCH/);
  assert.match(text, /[Dd]o(?:es)? not invent work/);
  assert.match(text, /git worktree/);
  assert.match(text, /actor cmd/);
  assert.match(text, /never required/);
  assert.equal(/\n {2}mint:/m.test(text), false);
  const review = cfg.lanes.find((lane) => lane.lane === "review");
  assert.equal(review?.type, "single");
  if (review?.type !== "single") throw new Error("expected single");
  assert.equal(review.scalars["mint-status"], "ready-for-human");
  assert.deepEqual(review.cmd, OPENCODE_CMD);
  console.log("hivemind.template:review-actor-gates");
});

test("hivemind.dogfood:opencode-cmd", () => {
  const root = WORKBENCH;
  const dest = loadConfig(root);
  const pack = loadConfigFile({ file: FIXTURE });
  for (const lane of [...dest.lanes, ...pack.lanes]) {
    assert.equal(lane.type, "single");
    if (lane.type !== "single") throw new Error("expected single");
    assert.deepEqual(lane.cmd, OPENCODE_CMD);
    assert.notEqual(lane.cmd[0], "pi");
    assert.equal(typeof lane.prompt, "string");
    assert.match(lane.prompt ?? "", /^\.opencode\/prompts\//);
    assert.ok(
      existsSync(join(root, lane.prompt ?? "")),
      `prompt file missing: ${lane.prompt}`,
    );
  }
  const destText = readFileSync(DEST_YAML, "utf8");
  const packText = readFileSync(FIXTURE, "utf8");
  assert.match(destText, /^ {2}opencode:/m);
  assert.match(packText, /^ {2}opencode:/m);
  assert.doesNotMatch(destText, /^ {2}pi:/m);
  assert.doesNotMatch(packText, /^ {2}pi:/m);
  console.log("hivemind.dogfood:opencode-cmd");
});

test("dest yaml runs the endless improvement loop on the heartbeat note", () => {
  const root = WORKBENCH;
  const cfg = loadConfig(root);
  const byId = new Map(cfg.lanes.map((lane) => [lane.lane, lane]));
  const audit = byId.get("audit");
  const shape = byId.get("shape");
  const keep = byId.get("keep");
  assert.ok(audit && shape && keep, "audit, shape, and keep lanes exist");
  assert.ok(byId.has("drain"), "drain lane stays");
  assert.equal(cfg.history, ".heio/logs/hivemind.tsv");

  // Phase chain: stoke -> audited -> planned -> stoke.
  assert.deepEqual(audit.trigger, { kind: "cycle", status: "stoke" });
  assert.deepEqual(shape.trigger, { kind: "cycle", status: "audited" });
  assert.deepEqual(keep.trigger, { kind: "cycle", status: "planned" });

  // Distinct claim statuses keep ttl reverts lane-local: the heartbeat can
  // never be reverted to a slice trigger status by the drain lane.
  const claims = [
    audit.claimStatus,
    shape.claimStatus,
    keep.claimStatus,
    byId.get("drain")?.claimStatus,
  ];
  assert.equal(new Set(claims).size, claims.length, "claim statuses unique");
  assert.ok(audit.ttlMs > 0 && shape.ttlMs > 0 && keep.ttlMs > 0);
  assert.ok(audit.cooldownMs > 0, "audit cooldown paces the loop");

  // Cycle lanes hold the same exclusive set as drain, so one seat runs at a time.
  const drain = byId.get("drain");
  for (const lane of [audit, shape, keep]) {
    assert.deepEqual(lane.exclusive, drain?.exclusive);
  }

  // Agent prompt files exist, or the lane silently skips on missing-prompt.
  const prompts = [audit, shape, keep]
    .map((lane) => lane.prompt)
    .filter((p) => p !== undefined) as string[];
  assert.equal(prompts.length, 3);
  for (const rel of prompts) {
    assert.ok(existsSync(join(root, rel)), `prompt file missing: ${rel}`);
  }

  // Heartbeat note is parked at rest with kind cycle.
  const heartbeat = readFileSync(
    join(root, ".heio/planning/loop/cycle.md"),
    "utf8",
  );
  assert.match(heartbeat, /^kind: "cycle"$/m);
  assert.match(heartbeat, /^status: "rest"$/m);

  // The endless entrypoint exists.
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
    scripts?: Record<string, string>;
  };
  assert.equal(
    pkg.scripts?.["hivemind:loop"],
    "node --experimental-strip-types scripts/loop.mjs",
  );
  assert.ok(existsSync(join(root, "scripts/loop.mjs")));
});

test("loadConfig omits watch as folders when the key is absent", () => {
  const cwd = withTemp("hivemind-load-watch-omit-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  assert.equal(loadConfig(cwd).watch, undefined);
});

test("loadConfig stores watch directories and strips glob suffixes", () => {
  const cwd = withTemp("hivemind-load-watch-");
  writeConfig(
    cwd,
    "folders: []\nlanes: {}\nwatch:\n  - .heio/tickets\n  - .heio/planning/**/*.md\n",
  );
  assert.deepEqual(loadConfig(cwd).watch, [".heio/tickets", ".heio/planning"]);
});

test("loadConfig omits history when the key is absent", () => {
  const cwd = withTemp("hivemind-load-history-omit-");
  writeConfig(cwd, "folders: []\nlanes: {}\n");
  assert.equal(loadConfig(cwd).history, undefined);
});

test("loadConfig stores a history path", () => {
  const cwd = withTemp("hivemind-load-history-");
  writeConfig(
    cwd,
    "folders: []\nlanes: {}\nhistory: .heio/logs/hivemind.tsv\n",
  );
  assert.equal(loadConfig(cwd).history, ".heio/logs/hivemind.tsv");
});

test("loadConfig rejects unknown lane type", () => {
  const cwd = withTemp("hivemind-load-type-");
  writeConfig(cwd, "folders: []\nlanes:\n  plan:\n    type: banana\n");
  assert.throws(() => loadConfig(cwd), /unknown type "banana"/);
});

test("loadConfig stores disable lane ids", () => {
  const cwd = withTemp("hivemind-load-disable-");
  writeConfig(cwd, "folders: []\nlanes: {}\ndisable: [mint, doctor]\n");
  assert.deepEqual(loadConfig(cwd).disable, ["mint", "doctor"]);
});

test("loadConfig omits lane ttl as ttlMs=0", () => {
  const cwd = withTemp("hivemind-load-ttl-omit-");
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
      "",
    ].join("\n"),
  );
  assert.equal(loadConfig(cwd).lanes[0]?.ttlMs, 0);
});

test('loadConfig stores ttl 0 and "0" as ttlMs=0', () => {
  const zero = withTemp("hivemind-load-ttl-zero-");
  writeConfig(
    zero,
    [
      "folders: []",
      "lanes:",
      "  plan:",
      "    type: single",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: ready",
      "    claim-status: active",
      "    ttl: 0",
      "",
    ].join("\n"),
  );
  assert.equal(loadConfig(zero).lanes[0]?.ttlMs, 0);

  const quoted = withTemp("hivemind-load-ttl-quoted-zero-");
  writeConfig(
    quoted,
    [
      "folders: []",
      "lanes:",
      "  plan:",
      "    type: single",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: ready",
      "    claim-status: active",
      '    ttl: "0"',
      "",
    ].join("\n"),
  );
  assert.equal(loadConfig(quoted).lanes[0]?.ttlMs, 0);
});

test("loadConfig parses lane timeout and stall durations", () => {
  const cwd = withTemp("hivemind-load-timeout-stall-");
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
      "    timeout: 30s",
      "    stall: 5m",
      "",
    ].join("\n"),
  );
  const lane = loadConfig(cwd).lanes[0];
  assert.equal(lane?.timeoutMs, 30_000);
  assert.equal(lane?.stallMs, 300_000);
});

test("loadConfig parses lane ttl 30m and 1h", () => {
  const minutes = withTemp("hivemind-load-ttl-30m-");
  writeConfig(
    minutes,
    [
      "folders: []",
      "history: .heio/logs/hivemind.tsv",
      "lanes:",
      "  plan:",
      "    type: single",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: ready",
      "    claim-status: active",
      "    ttl: 30m",
      "",
    ].join("\n"),
  );
  assert.equal(loadConfig(minutes).lanes[0]?.ttlMs, 1_800_000);

  const hours = withTemp("hivemind-load-ttl-1h-");
  writeConfig(
    hours,
    [
      "folders: []",
      "history: .heio/logs/hivemind.tsv",
      "lanes:",
      "  plan:",
      "    type: single",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: ready",
      "    claim-status: active",
      "    ttl: 1h",
      "",
    ].join("\n"),
  );
  assert.equal(loadConfig(hours).lanes[0]?.ttlMs, 3_600_000);
});

test("loadConfig rejects positive ttl without history", () => {
  const cwd = withTemp("hivemind-load-ttl-no-history-");
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
      "    ttl: 30m",
      "",
    ].join("\n"),
  );
  assert.throws(() => loadConfig(cwd), /ttl requires history/);
});

test("loadConfig rejects illegal lane ttl", () => {
  const illegal = ["30", '"30"', "1d", "30M", "true", '""'];
  for (const value of illegal) {
    const cwd = withTemp("hivemind-load-ttl-illegal-");
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
    assert.throws(() => loadConfig(cwd), /ttl is invalid/, `ttl: ${value}`);
  }
});

test("hivemind.config:cmd-opts-out: pipeline actor prompt is unset on a cmd-only stage", () => {
  const cwd = withTemp("hivemind-cmd-opts-pipe-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "actors:",
      "  pi:",
      "    cmd: /bin/echo actor",
      "    agent: heio-planner",
      "    prompt: .pi/prompts/heio-planning.md",
      "    exclusive:",
      "      - .heio/planning",
      "    claim-status: active",
      "    mint-status: from-actor",
      "lanes:",
      "  workflow:",
      "    type: pipeline",
      "    actor: pi",
      "    trigger:",
      "      status: ready",
      "    stages:",
      "      - stage: lint",
      "        cmd: /bin/echo lint",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  const lane = cfg.lanes[0];
  assert.equal(lane?.type, "pipeline");
  if (lane?.type !== "pipeline") throw new Error("expected pipeline");
  const stage = lane.stages[0];
  assert.equal(stage?.cmd, "/bin/echo lint");
  assert.equal(stage?.agent, undefined);
  assert.equal(stage?.prompt, undefined);
  assert.deepEqual(stage?.exclusive, [".heio/planning"]);
  assert.equal(stage?.claimStatus, "active");
  assert.equal(stage?.scalars["mint-status"], "from-actor");
});

test("hivemind.config:cmd-opts-out: cmd-only stage keeps explicit agent", () => {
  const cwd = withTemp("hivemind-cmd-opts-agent-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "actors:",
      "  pi:",
      "    cmd: /bin/echo actor",
      "    agent: heio-planner",
      "    prompt: .pi/prompts/heio-planning.md",
      "    claim-status: active",
      "lanes:",
      "  workflow:",
      "    type: pipeline",
      "    actor: pi",
      "    trigger:",
      "      status: ready",
      "    stages:",
      "      - stage: build",
      "        cmd: /bin/echo build",
      "        agent: heio-builder",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  const lane = cfg.lanes[0];
  assert.equal(lane?.type, "pipeline");
  if (lane?.type !== "pipeline") throw new Error("expected pipeline");
  const stage = lane.stages[0];
  assert.equal(stage?.agent, "heio-builder");
  assert.equal(stage?.prompt, undefined);
});

test("hivemind.config:cmd-opts-out: single with actor and cmd does not inherit prompt", () => {
  const cwd = withTemp("hivemind-cmd-opts-single-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "actors:",
      "  pi:",
      "    cmd: /bin/echo actor",
      "    agent: heio-planner",
      "    prompt: .pi/prompts/heio-planning.md",
      "    exclusive:",
      "      - .heio/planning",
      "    claim-status: active",
      "lanes:",
      "  unit:",
      "    type: single",
      "    actor: pi",
      "    cmd: /bin/echo unit",
      "    trigger:",
      "      status: ready",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  const lane = cfg.lanes[0];
  assert.equal(lane?.type, "single");
  if (lane?.type !== "single") throw new Error("expected single");
  assert.equal(lane.cmd, "/bin/echo unit");
  assert.equal(lane.agent, undefined);
  assert.equal(lane.prompt, undefined);
  assert.deepEqual(lane.exclusive, [".heio/planning"]);
  assert.equal(lane.claimStatus, "active");
});

test("hivemind.config:cmd-opts-out: omit cmd still inherits actor cmd agent prompt", () => {
  const cwd = withTemp("hivemind-cmd-opts-inherit-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "actors:",
      "  pi:",
      "    cmd: /bin/echo actor",
      "    agent: heio-planner",
      "    prompt: .pi/prompts/heio-planning.md",
      "    claim-status: active",
      "lanes:",
      "  unit:",
      "    type: single",
      "    actor: pi",
      "    trigger:",
      "      status: ready",
      "  workflow:",
      "    type: pipeline",
      "    actor: pi",
      "    trigger:",
      "      status: ready",
      "    stages:",
      "      - stage: plan",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  const unit = cfg.lanes.find((lane) => lane.lane === "unit");
  const workflow = cfg.lanes.find((lane) => lane.lane === "workflow");
  assert.equal(unit?.type, "single");
  if (unit?.type !== "single") throw new Error("expected single");
  assert.equal(unit.cmd, "/bin/echo actor");
  assert.equal(unit.agent, "heio-planner");
  assert.equal(unit.prompt, ".pi/prompts/heio-planning.md");
  assert.equal(workflow?.type, "pipeline");
  if (workflow?.type !== "pipeline") throw new Error("expected pipeline");
  const stage = workflow.stages[0];
  assert.equal(stage?.cmd, "/bin/echo actor");
  assert.equal(stage?.agent, "heio-planner");
  assert.equal(stage?.prompt, ".pi/prompts/heio-planning.md");
});

test("hivemind.config:cmd-opts-out: pipeline-default agent prompt is unset on a cmd-only stage", () => {
  const cwd = withTemp("hivemind-cmd-opts-defaults-");
  writeConfig(
    cwd,
    [
      "folders: []",
      "lanes:",
      "  workflow:",
      "    type: pipeline",
      "    cmd: /bin/echo default",
      "    agent: from-pipeline",
      "    prompt: pipeline.md",
      "    claim-status: active",
      "    trigger:",
      "      status: ready",
      "    stages:",
      "      - stage: lint",
      "        cmd: /bin/echo lint",
      "      - stage: plan",
      "",
    ].join("\n"),
  );
  const cfg = loadConfig(cwd);
  const lane = cfg.lanes[0];
  assert.equal(lane?.type, "pipeline");
  if (lane?.type !== "pipeline") throw new Error("expected pipeline");
  const lint = lane.stages[0];
  const plan = lane.stages[1];
  assert.equal(lint?.cmd, "/bin/echo lint");
  assert.equal(lint?.agent, undefined);
  assert.equal(lint?.prompt, undefined);
  assert.equal(plan?.cmd, "/bin/echo default");
  assert.equal(plan?.agent, "from-pipeline");
  assert.equal(plan?.prompt, "pipeline.md");
});

test("hivemind.config:cmd-opts-out: unknown before script kind fail closed", () => {
  for (const key of ["before", "script", "kind"]) {
    const single = withTemp(`hivemind-cmd-opts-single-${key}-`);
    writeConfig(
      single,
      [
        "folders: []",
        "lanes:",
        "  unit:",
        "    type: single",
        "    cmd: /bin/echo",
        "    claim-status: active",
        "    trigger:",
        "      status: ready",
        `    ${key}:`,
        "      cmd: /bin/true",
        "",
      ].join("\n"),
    );
    assert.throws(
      () => loadConfig(single),
      new RegExp(`unknown key "${key}"`),
      `single ${key}`,
    );

    const pipe = withTemp(`hivemind-cmd-opts-stage-${key}-`);
    writeConfig(
      pipe,
      [
        "folders: []",
        "lanes:",
        "  workflow:",
        "    type: pipeline",
        "    claim-status: active",
        "    trigger:",
        "      status: ready",
        "    stages:",
        "      - stage: lint",
        "        cmd: /bin/echo lint",
        `        ${key}:`,
        "          cmd: /bin/true",
        "",
      ].join("\n"),
    );
    assert.throws(
      () => loadConfig(pipe),
      new RegExp(`unknown key "${key}"`),
      `stage ${key}`,
    );
  }
});

test("hivemind.config:claim-status-eq-trigger: loadConfig throws when claim-status equals trigger.status", () => {
  const single = withTemp("hivemind-claim-eq-single-");
  writeConfig(
    single,
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
  assert.throws(
    () => loadConfig(single),
    /claim-status equals trigger\.status/,
  );

  const pipe = withTemp("hivemind-claim-eq-pipe-");
  writeConfig(
    pipe,
    [
      "folders: []",
      "lanes:",
      "  workflow:",
      "    type: pipeline",
      "    claim-status: ready",
      "    trigger:",
      "      status: ready",
      "    stages:",
      "      - stage: lint",
      "        cmd: /bin/echo lint",
      "",
    ].join("\n"),
  );
  assert.throws(() => loadConfig(pipe), /claim-status equals trigger\.status/);
});

test("hivemind.config:lane-audit", () => {
  const single = (extra = "") =>
    [
      "folders: []",
      "lanes:",
      "  plan:",
      "    type: single",
      "    cmd: /bin/echo",
      "    trigger:",
      "      status: ready",
      "    claim-status: active",
      extra,
    ].join("\n");
  const opted = withTemp("hivemind-load-audit-git-diff-");
  writeConfig(opted, single("    audit: git-diff"));
  const optedLane = loadConfig(opted).lanes[0];
  assert.equal(optedLane?.audit, "git-diff");
  assert.equal(optedLane?.scalars.audit, undefined);

  const omitted = withTemp("hivemind-load-audit-omit-");
  writeConfig(omitted, single());
  const omittedLane = loadConfig(omitted).lanes[0];
  assert.equal(omittedLane?.audit, undefined);
  assert.equal(omittedLane?.scalars.audit, undefined);

  const pipe = withTemp("hivemind-load-audit-pipe-");
  writeConfig(
    pipe,
    [
      "folders: []",
      "lanes:",
      "  workflow:",
      "    type: pipeline",
      "    claim-status: active",
      "    trigger:",
      "      status: ready",
      "    audit: git-diff",
      "    stages:",
      "      - stage: lint",
      "        cmd: /bin/echo lint",
      "",
    ].join("\n"),
  );
  assert.equal(loadConfig(pipe).lanes[0]?.audit, "git-diff");

  for (const value of ["git", "true", "1", '""']) {
    const cwd = withTemp("hivemind-load-audit-unknown-");
    writeConfig(cwd, single(`    audit: ${value}`));
    assert.throws(() => loadConfig(cwd), /audit/, `audit: ${value}`);
  }

  const top = withTemp("hivemind-load-audit-top-");
  writeConfig(top, "folders: []\nlanes: {}\naudit: git-diff\n");
  assert.throws(() => loadConfig(top), /Unknown key "audit"/);

  const actor = withTemp("hivemind-load-audit-actor-");
  writeConfig(
    actor,
    [
      "folders: []",
      "actors:",
      "  pi:",
      "    cmd: /bin/echo",
      "    claim-status: active",
      "    audit: git-diff",
      "lanes:",
      "  unit:",
      "    type: single",
      "    actor: pi",
      "    trigger:",
      "      status: ready",
      "",
    ].join("\n"),
  );
  assert.throws(() => loadConfig(actor), /unknown key "audit"/);
  console.log("hivemind.config:lane-audit");
});
