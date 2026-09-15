import assert from "node:assert/strict";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { withTemp } from "../with-temp.ts";

const CLI = fileURLToPath(
  new URL("../../src/cli.ts", import.meta.url),
);

function writeConfig(cwd: string, lines: string[]): void {
  mkdirSync(join(cwd, ".hivemind"), { recursive: true });
  writeFileSync(join(cwd, ".hivemind", "hivemind.yaml"), lines.join("\n"));
}

function setupEmptyProject(): string {
  const cwd = withTemp("hivemind-watch-");
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
    "    cmd: /bin/echo",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  return cwd;
}

test("watch --until-quiet on an empty match set exits after one quiet scan", () => {
  const cwd = setupEmptyProject();
  const proc = spawnSync(
    process.execPath,
    ["--experimental-strip-types", CLI, "watch", "--until-quiet"],
    { cwd, encoding: "utf8", timeout: 5000 },
  );
  assert.equal(proc.status, 0, proc.stderr);
  assert.equal(proc.signal, null);
});

test("--until-target PATH exits when PATH exists", async () => {
  const cwd = setupEmptyProject();
  const target = join(cwd, "target.flag");
  const child = spawn(
    process.execPath,
    [
      "--experimental-strip-types",
      CLI,
      "watch",
      "--until-target",
      "target.flag",
    ],
    { cwd },
  );
  const exited = new Promise<{ status: number | null; stderr: string }>(
    (resolve) => {
      let stderr = "";
      child.stderr.on("data", (chunk: Buffer | string) => {
        stderr += chunk.toString();
      });
      child.on("close", (status) => {
        resolve({ status, stderr });
      });
    },
  );
  try {
    await new Promise((resolve) => setTimeout(resolve, 150));
    writeFileSync(target, "1");
    const proc = await withTimeout(exited, 4000);
    assert.equal(proc.status, 0, proc.stderr);
  } finally {
    child.kill("SIGKILL");
  }
});

test("hivemind.loop:stop-drain: STOP file drains live children then exits 0 with no new claims", async () => {
  const cwd = withTemp("hivemind-watch-stop-drain-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "hold.mjs"),
    [
      "import { appendFileSync, existsSync } from 'node:fs';",
      "appendFileSync('spawns.log', process.argv[2] + '\\n');",
      "while (!existsSync('release.flag')) {",
      "  await new Promise((resolve) => setTimeout(resolve, 20));",
      "}",
      "appendFileSync('finished.log', process.argv[2] + '\\n');",
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
    "      - hold.mjs",
    '      - "{{path}}"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeFileSync(
    join(cwd, "tickets", "one.md"),
    "---\nid: one\nstatus: ready-for-agent\n---\n\n# one\n",
  );
  const child = spawn(
    process.execPath,
    ["--experimental-strip-types", CLI, "watch"],
    { cwd },
  );
  const exited = new Promise<{ status: number | null; stderr: string }>(
    (resolve) => {
      let stderr = "";
      child.stderr.on("data", (chunk: Buffer | string) => {
        stderr += chunk.toString();
      });
      child.on("close", (status) => {
        resolve({ status, stderr });
      });
    },
  );
  try {
    await waitForPath(join(cwd, "spawns.log"), 4000);
    writeFileSync(join(cwd, ".hivemind", "STOP"), "");
    writeFileSync(
      join(cwd, "tickets", "two.md"),
      "---\nid: two\nstatus: ready-for-agent\n---\n\n# two\n",
    );
    await delay(400);
    assert.match(
      readFileSync(join(cwd, "tickets", "one.md"), "utf8"),
      /status: active/,
    );
    assert.match(
      readFileSync(join(cwd, "tickets", "two.md"), "utf8"),
      /status: ready-for-agent/,
    );
    assert.equal(existsSync(join(cwd, "finished.log")), false);
    writeFileSync(join(cwd, "release.flag"), "1");
    const proc = await withTimeout(exited, 4000);
    assert.equal(proc.status, 0, proc.stderr);
    assert.equal(existsSync(join(cwd, "finished.log")), true);
    assert.equal(
      readFileSync(join(cwd, "spawns.log"), "utf8"),
      "tickets/one.md\n",
    );
  } finally {
    child.kill("SIGKILL");
  }
});

test("hivemind.config:reload-future: dest yaml change applies to the next spawn; in-flight argv is unchanged", async () => {
  const cwd = withTemp("hivemind-watch-reload-future-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "hold.mjs"),
    [
      "import { appendFileSync, existsSync } from 'node:fs';",
      "appendFileSync('spawns.log', process.argv.slice(2).join(' ') + '\\n');",
      "while (!existsSync('release.flag')) {",
      "  await new Promise((resolve) => setTimeout(resolve, 20));",
      "}",
      "",
    ].join("\n"),
  );
  const yaml = (token: string): string[] => [
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
    "      - hold.mjs",
    `      - ${token}`,
    '      - "{{path}}"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ];
  writeConfig(cwd, yaml("old"));
  writeFileSync(
    join(cwd, "tickets", "one.md"),
    "---\nid: one\nstatus: ready-for-agent\n---\n\n# one\n",
  );
  const child = spawn(
    process.execPath,
    ["--experimental-strip-types", CLI, "watch"],
    { cwd },
  );
  try {
    await waitForPath(join(cwd, "spawns.log"), 4000);
    assert.equal(
      readFileSync(join(cwd, "spawns.log"), "utf8"),
      "old tickets/one.md\n",
    );
    writeConfig(cwd, yaml("new"));
    writeFileSync(
      join(cwd, "tickets", "two.md"),
      "---\nid: two\nstatus: ready-for-agent\n---\n\n# two\n",
    );
    await waitForText(join(cwd, "spawns.log"), "new tickets/two.md\n", 4000);
    assert.equal(
      readFileSync(join(cwd, "spawns.log"), "utf8"),
      "old tickets/one.md\nnew tickets/two.md\n",
    );
  } finally {
    writeFileSync(join(cwd, "release.flag"), "1");
    child.kill("SIGKILL");
    await onceClose(child);
  }
});

test("hivemind.config:reload-future: illegal yaml fails closed and is not silently ignored", async () => {
  const cwd = withTemp("hivemind-watch-reload-illegal-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "hold.mjs"),
    [
      "import { appendFileSync, existsSync } from 'node:fs';",
      "appendFileSync('spawns.log', process.argv[2] + '\\n');",
      "while (!existsSync('release.flag')) {",
      "  await new Promise((resolve) => setTimeout(resolve, 20));",
      "}",
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
    "      - hold.mjs",
    '      - "{{path}}"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeFileSync(
    join(cwd, "tickets", "one.md"),
    "---\nid: one\nstatus: ready-for-agent\n---\n\n# one\n",
  );
  const child = spawn(
    process.execPath,
    ["--experimental-strip-types", CLI, "watch"],
    { cwd },
  );
  const exited = new Promise<{ status: number | null; stderr: string }>(
    (resolve) => {
      let stderr = "";
      child.stderr.on("data", (chunk: Buffer | string) => {
        stderr += chunk.toString();
      });
      child.on("close", (status) => {
        resolve({ status, stderr });
      });
    },
  );
  try {
    await waitForPath(join(cwd, "spawns.log"), 4000);
    writeConfig(cwd, ["folders: []", "lanes: []", "unknown: 1", ""]);
    writeFileSync(
      join(cwd, "tickets", "two.md"),
      "---\nid: two\nstatus: ready-for-agent\n---\n\n# two\n",
    );
    const proc = await withTimeout(exited, 4000);
    assert.equal(proc.status, 1, proc.stderr);
    assert.match(proc.stderr, /must be a map|Unknown key/);
    assert.equal(
      readFileSync(join(cwd, "spawns.log"), "utf8"),
      "tickets/one.md\n",
    );
    assert.match(
      readFileSync(join(cwd, "tickets", "two.md"), "utf8"),
      /status: ready-for-agent/,
    );
  } finally {
    writeFileSync(join(cwd, "release.flag"), "1");
    child.kill("SIGKILL");
  }
});

test("hivemind.loop:watch-recursive: a write under a scanned subdirectory wakes watch", async () => {
  const cwd = withTemp("hivemind-watch-nested-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "tickets", "nested"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "hold.mjs"),
    [
      "import { appendFileSync } from 'node:fs';",
      "appendFileSync('spawns.log', process.argv[2] + '\\n');",
      "await new Promise((resolve) => setTimeout(resolve, 30000));",
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
    "      - hold.mjs",
    '      - "{{path}}"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "    backoff: 20s",
    "",
  ]);
  writeFileSync(
    join(cwd, "tickets", "one.md"),
    "---\nid: one\nstatus: ready-for-agent\n---\n\n# one\n",
  );
  const child = startWatch(cwd);
  try {
    await waitForPath(join(cwd, "spawns.log"), 4000);
    assert.equal(
      readFileSync(join(cwd, "spawns.log"), "utf8"),
      "tickets/one.md\n",
    );
    await delay(150);
    writeFileSync(
      join(cwd, "tickets", "nested", "two.md"),
      "---\nid: two\nstatus: ready-for-agent\n---\n\n# two\n",
    );
    await waitForText(join(cwd, "spawns.log"), "tickets/nested/two.md\n", 3000);
  } finally {
    child.kill("SIGTERM");
    await onceClose(child);
  }
});

test("backoff does not busy-spin; killing the process stops spawn", async () => {
  const cwd = withTemp("hivemind-watch-backoff-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "record.mjs"),
    "import { appendFileSync } from 'node:fs';\nappendFileSync('spawns.log', '1\\n');\n",
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
    "    backoff: 1s",
    "",
  ]);
  const child = startWatch(cwd);
  try {
    await delay(250);
    writeFileSync(
      join(cwd, "tickets", "agent.md"),
      "---\nid: agent\nstatus: ready-for-agent\n---\n\n# agent\n",
    );
    await delay(400);
    assert.equal(existsSync(join(cwd, "spawns.log")), false);
    await waitForPath(join(cwd, "spawns.log"), 2000);
  } finally {
    child.kill("SIGTERM");
    await onceClose(child);
  }
});

test("hivemind.cli:max-spawns watch --max-spawns N stops new claims after N spawns", async () => {
  const cwd = withTemp("hivemind-watch-max-spawns-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "hold.mjs"),
    [
      "import { appendFileSync } from 'node:fs';",
      "appendFileSync('spawns.log', process.argv[2] + '\\n');",
      "await new Promise((resolve) => setTimeout(resolve, 30000));",
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
    "      - hold.mjs",
    '      - "{{path}}"',
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  writeFileSync(
    join(cwd, "tickets", "one.md"),
    "---\nid: one\nstatus: ready-for-agent\n---\n\n# one\n",
  );
  writeFileSync(
    join(cwd, "tickets", "two.md"),
    "---\nid: two\nstatus: ready-for-agent\n---\n\n# two\n",
  );
  const child = startWatch(cwd, ["--max-spawns", "1"]);
  try {
    await waitForPath(join(cwd, "spawns.log"), 4000);
    await delay(300);
    const spawned = readFileSync(join(cwd, "spawns.log"), "utf8")
      .trim()
      .split("\n")
      .filter((line) => line !== "");
    assert.equal(spawned.length, 1);
    const one = readFileSync(join(cwd, "tickets", "one.md"), "utf8");
    const two = readFileSync(join(cwd, "tickets", "two.md"), "utf8");
    const claimed = [one, two].filter((text) => /status: active/.test(text));
    const ready = [one, two].filter((text) =>
      /status: ready-for-agent/.test(text),
    );
    assert.equal(claimed.length, 1);
    assert.equal(ready.length, 1);
  } finally {
    child.kill("SIGTERM");
    await onceClose(child);
  }
});

test("killing the process stops spawn", async () => {
  const cwd = withTemp("hivemind-watch-kill-");
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "hold.mjs"),
    [
      "import { writeFileSync } from 'node:fs';",
      "writeFileSync('child.pid', String(process.pid));",
      "await new Promise((resolve) => setTimeout(resolve, 30000));",
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
    "      - hold.mjs",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "    backoff: 1s",
    "",
  ]);
  writeFileSync(
    join(cwd, "tickets", "agent.md"),
    "---\nid: agent\nstatus: ready-for-agent\n---\n\n# agent\n",
  );
  const child = startWatch(cwd);
  try {
    await waitForPath(join(cwd, "child.pid"), 4000);
    const spawnedPid = Number(readFileSync(join(cwd, "child.pid"), "utf8"));
    assert.equal(Number.isInteger(spawnedPid) && spawnedPid > 0, true);
    child.kill("SIGTERM");
    await onceClose(child);
    await delay(200);
    assert.equal(pidAlive(spawnedPid), false);
  } finally {
    child.kill("SIGKILL");
  }
});

function startWatch(cwd: string, extra: readonly string[] = []): ChildProcess {
  return spawn(
    process.execPath,
    ["--experimental-strip-types", CLI, "watch", ...extra],
    { cwd },
  );
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function onceClose(child: ChildProcess): Promise<void> {
  return withTimeout(
    new Promise((resolve) => {
      if (child.exitCode !== null || child.signalCode !== null) {
        resolve();
        return;
      }
      child.on("close", () => resolve());
    }),
    5000,
  );
}

async function waitForPath(path: string, ms: number): Promise<void> {
  const start = Date.now();
  while (!existsSync(path)) {
    if (Date.now() - start > ms) {
      throw new Error(`timed out waiting for ${path}`);
    }
    await delay(20);
  }
}

async function waitForText(
  path: string,
  needle: string,
  ms: number,
): Promise<void> {
  const start = Date.now();
  while (true) {
    if (existsSync(path) && readFileSync(path, "utf8").includes(needle)) {
      return;
    }
    if (Date.now() - start > ms) {
      throw new Error(`timed out waiting for ${needle} in ${path}`);
    }
    await delay(20);
  }
}

function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`timed out after ${ms}ms`));
    }, ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err: unknown) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}
