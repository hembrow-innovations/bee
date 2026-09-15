import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import { loadConfig } from "../../src/config/loadConfig.ts";
import type {
  HistoryEvent,
  Journal,
} from "../../src/journal/journal.ts";
import { runTick } from "../../src/loop/tick.ts";

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

function writeConfig(cwd: string, lines: string[]): void {
  mkdirSync(join(cwd, ".hivemind"), { recursive: true });
  writeFileSync(join(cwd, ".hivemind", "hivemind.yaml"), lines.join("\n"));
}

function setupProject(): string {
  const cwd = withTemp("hivemind-tick-");
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
    "    cmd: /bin/echo agent-matched",
    "    trigger:",
    "      status: ready-for-agent",
    "    claim-status: active",
    "",
  ]);
  return cwd;
}

function tick(cwd: string): {
  spawned: number;
  events: HistoryEvent[];
} {
  const config = loadConfig(cwd);
  const { journal, events } = memoryJournal();
  const spawned = runTick({
    cwd,
    config,
    lanes: config.lanes,
    env: {},
    live: [],
    journal,
    spawnChild: () => ({ wait: Promise.resolve(0), kill: () => {} }),
  });
  return { spawned, events };
}

test("runTick records an empty scan and spawns nothing", () => {
  const cwd = setupProject();
  const { spawned, events } = tick(cwd);
  assert.equal(spawned, 0);
  assert.deepEqual(events, [{ kind: "scan", notes: 0, quarantined: 0 }]);
});

test("runTick journals scan and quarantine events", () => {
  const cwd = setupProject();
  writeFileSync(join(cwd, "tickets", "bad.md"), "---\nid: bad\n---\n");
  const { spawned, events } = tick(cwd);
  assert.equal(spawned, 0);
  assert.deepEqual(events, [
    { kind: "scan", notes: 0, quarantined: 1 },
    {
      kind: "quarantine",
      path: "tickets/bad.md",
      fault: "missing-key:status",
    },
  ]);
});

test("runTick claims a matching note and returns the spawn count", () => {
  const cwd = setupProject();
  writeFileSync(
    join(cwd, "tickets", "agent.md"),
    "---\nid: agent\nstatus: ready-for-agent\n---\n\n# agent\n",
  );
  const { spawned, events } = tick(cwd);
  assert.equal(spawned, 1);
  assert.deepEqual(events[0], { kind: "scan", notes: 1, quarantined: 0 });
  assert.equal(
    events.some(
      (event) =>
        event.kind === "claim" &&
        event.lane === "plan" &&
        event.path === "tickets/agent.md",
    ),
    true,
  );
  assert.equal(
    events.some(
      (event) =>
        event.kind === "spawn" &&
        event.lane === "plan" &&
        event.path === "tickets/agent.md",
    ),
    true,
  );
  const claimed = readFileSync(join(cwd, "tickets", "agent.md"), "utf8");
  assert.match(claimed, /^status: active$/m);
  assert.match(claimed, /^claimed-by: /m);
});
