import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import type { UnitLane } from "../../src/config/loadConfig.ts";
import type {
  HistoryEvent,
  Journal,
} from "../../src/journal/journal.ts";
import { matchNotes } from "../../src/match/matcher.ts";

function lane(extra?: Partial<UnitLane>): UnitLane {
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

test("matchNotes requires every trigger key and ignores extra front matter", () => {
  const plan = lane({ trigger: { status: "ready-for-agent", kind: "ticket" } });
  const matches = matchNotes({
    lanes: [plan],
    notes: [
      {
        path: "tickets/extra.md",
        frontMatter: {
          status: "ready-for-agent",
          kind: "ticket",
          extra: "ok",
        },
      },
      {
        path: "tickets/partial.md",
        frontMatter: { status: "ready-for-agent" },
      },
    ],
  });
  assert.deepEqual(
    matches.map((item) => item.note.path),
    ["tickets/extra.md"],
  );
});

test("matchNotes skips a note when need fails and does not fault it", () => {
  const plan = lane({ need: { sealed: true } });
  const notes = [
    {
      path: "tickets/open.md",
      frontMatter: { status: "ready-for-agent" },
    },
    {
      path: "tickets/sealed.md",
      frontMatter: { status: "ready-for-agent", sealed: true },
    },
  ];
  const matches = matchNotes({ lanes: [plan], notes });
  assert.deepEqual(
    matches.map((item) => item.note.path),
    ["tickets/sealed.md"],
  );
  assert.deepEqual(
    notes.map((note) => note.path),
    ["tickets/open.md", "tickets/sealed.md"],
  );
});

test("matchNotes omits disabled lane ids", () => {
  const plan = lane();
  const build = lane({ lane: "build" });
  const matches = matchNotes({
    lanes: [plan, build],
    notes: [
      {
        path: "tickets/agent.md",
        frontMatter: { status: "ready-for-agent" },
      },
    ],
    disable: ["plan"],
  });
  assert.deepEqual(
    matches.map((item) => item.lane.lane),
    ["build"],
  );
});

test("hivemind.match:need-exists", () => {
  const cwd = withTemp("hivemind-need-exists-");
  writeFileSync(join(cwd, "present.md"), "sealed\n");
  const notes = [
    {
      path: "tickets/open.md",
      frontMatter: { status: "ready-for-agent" },
    },
  ];
  const hit = matchNotes({
    lanes: [lane({ need: { exists: ["present.md"], absent: ["gone.md"] } })],
    notes,
    cwd,
  });
  assert.deepEqual(
    hit.map((item) => item.note.path),
    ["tickets/open.md"],
  );
  const { journal, events } = memoryJournal();
  const miss = matchNotes({
    lanes: [lane({ need: { exists: ["missing.md"] } })],
    notes,
    cwd,
    journal,
  });
  assert.deepEqual(miss, []);
  assert.deepEqual(events, [
    {
      kind: "skip",
      lane: "plan",
      path: "tickets/open.md",
      reason: "need",
    },
  ]);
  const present = matchNotes({
    lanes: [lane({ need: { absent: ["present.md"] } })],
    notes,
    cwd,
  });
  assert.deepEqual(present, []);
  assert.deepEqual(
    notes.map((note) => note.path),
    ["tickets/open.md"],
  );
});

test("hivemind.match:need-status-of", () => {
  const notes = [
    {
      path: "tickets/ready.md",
      frontMatter: {
        id: "ready-note",
        status: "ready-for-agent",
        "blocked-by": "blocker",
      },
    },
    {
      path: "tickets/blocker.md",
      frontMatter: { id: "blocker", status: "completed" },
    },
    {
      path: "tickets/blocked.md",
      frontMatter: {
        id: "blocked-note",
        status: "ready-for-agent",
        "blocked-by": "open-blocker",
      },
    },
    {
      path: "tickets/open.md",
      frontMatter: { id: "open-blocker", status: "ready" },
    },
    {
      path: "tickets/orphan.md",
      frontMatter: {
        id: "orphan",
        status: "ready-for-agent",
        "blocked-by": "missing-id",
      },
    },
    {
      path: "tickets/clear.md",
      frontMatter: {
        id: "clear",
        status: "ready-for-agent",
        "blocked-by": "none",
      },
    },
    {
      path: "tickets/caused.md",
      frontMatter: {
        id: "caused",
        status: "ready-for-agent",
        "caused-by": "blocker",
      },
    },
  ];
  const { journal, events } = memoryJournal();
  const matches = matchNotes({
    lanes: [
      lane({
        need: {
          "status-of": { "blocked-by": "completed", "caused-by": "completed" },
        },
      }),
    ],
    notes,
    journal,
  });
  assert.deepEqual(
    matches.map((item) => item.note.path),
    ["tickets/ready.md", "tickets/clear.md", "tickets/caused.md"],
  );
  assert.equal(
    events.some(
      (event) => event.kind === "skip" && event.path === "tickets/orphan.md",
    ),
    true,
  );
  assert.deepEqual(
    notes.map((note) => note.path),
    [
      "tickets/ready.md",
      "tickets/blocker.md",
      "tickets/blocked.md",
      "tickets/open.md",
      "tickets/orphan.md",
      "tickets/clear.md",
      "tickets/caused.md",
    ],
  );
});

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
