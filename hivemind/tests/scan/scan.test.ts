import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import { scan } from "../../src/scan/scan.ts";

const NOW = new Date("2026-09-01T18:00:00.000Z");

const CONFIG = `folders:
  - path: notes
    schema:
      id: string
    required: [id]
  - path: quarantine
    schema: quarantine
    required: [origin-location, quarantined-at, fault]
lanes: {}
`;

function writeConfig(cwd: string, body: string): void {
  mkdirSync(join(cwd, ".hivemind"), { recursive: true });
  writeFileSync(join(cwd, ".hivemind", "hivemind.yaml"), body);
}

function setup(): string {
  const cwd = withTemp("hivemind-scan-");
  writeConfig(cwd, CONFIG);
  mkdirSync(join(cwd, "notes"));
  mkdirSync(join(cwd, "quarantine"));
  return cwd;
}

test("fault moves the file to configured quarantine with only origin-location, quarantined-at, fault", () => {
  const cwd = setup();
  writeFileSync(join(cwd, "notes", "bad.md"), "---\n{\n---\n");
  writeFileSync(
    join(cwd, "notes", "good.md"),
    "---\nid: good\n---\n\n# keep this body\n",
  );

  const result = scan({ cwd, now: NOW });

  assert.equal(existsSync(join(cwd, "notes", "bad.md")), false);
  const quarantined = readFileSync(join(cwd, "quarantine", "bad.md"), "utf8");
  assert.equal(
    quarantined,
    "---\norigin-location: notes/bad.md\nquarantined-at: 2026-09-01T18:00:00.000Z\nfault: parse-error\n---\n",
  );
  assert.equal(
    readFileSync(join(cwd, "notes", "good.md"), "utf8"),
    "---\nid: good\n---\n\n# keep this body\n",
  );
  assert.deepEqual(
    result.notes.map((note) => note.path),
    ["notes/good.md"],
  );
});

test("unknown key moves the file to quarantine with only three keys; scan continues", () => {
  const cwd = setup();
  writeFileSync(
    join(cwd, "notes", "extra.md"),
    "---\nid: extra\nmystery: 1\n---\n\n# body\n",
  );
  writeFileSync(
    join(cwd, "notes", "good.md"),
    "---\nid: good\n---\n\n# keep this body\n",
  );

  const result = scan({ cwd, now: NOW });

  assert.equal(existsSync(join(cwd, "notes", "extra.md")), false);
  assert.equal(
    readFileSync(join(cwd, "quarantine", "extra.md"), "utf8"),
    "---\norigin-location: notes/extra.md\nquarantined-at: 2026-09-01T18:00:00.000Z\nfault: unknown-key:mystery\n---\n",
  );
  assert.equal(
    readFileSync(join(cwd, "notes", "good.md"), "utf8"),
    "---\nid: good\n---\n\n# keep this body\n",
  );
  assert.deepEqual(
    result.notes.map((note) => note.path),
    ["notes/good.md"],
  );
});

test("missing required key moves the file to quarantine with only three keys; scan continues", () => {
  const cwd = setup();
  writeFileSync(join(cwd, "notes", "empty.md"), "---\n---\n\n# body\n");
  writeFileSync(
    join(cwd, "notes", "good.md"),
    "---\nid: good\n---\n\n# keep this body\n",
  );

  const result = scan({ cwd, now: NOW });

  assert.equal(existsSync(join(cwd, "notes", "empty.md")), false);
  assert.equal(
    readFileSync(join(cwd, "quarantine", "empty.md"), "utf8"),
    "---\norigin-location: notes/empty.md\nquarantined-at: 2026-09-01T18:00:00.000Z\nfault: missing-key:id\n---\n",
  );
  assert.equal(
    readFileSync(join(cwd, "notes", "good.md"), "utf8"),
    "---\nid: good\n---\n\n# keep this body\n",
  );
  assert.deepEqual(
    result.notes.map((note) => note.path),
    ["notes/good.md"],
  );
});

test("no status / blocked-by / caused-by / body written by the supervisor", () => {
  const cwd = setup();
  writeFileSync(
    join(cwd, "notes", "lineage.md"),
    [
      "---",
      "id: lineage",
      "status: ready-for-agent",
      "blocked-by: other",
      "caused-by: parent",
      "mystery: 1",
      "---",
      "",
      "# Body the supervisor must not copy",
      "status: still-a-body",
      "",
    ].join("\n"),
  );

  scan({ cwd, now: NOW });

  const quarantined = readFileSync(
    join(cwd, "quarantine", "lineage.md"),
    "utf8",
  );
  assert.equal(
    quarantined,
    "---\norigin-location: notes/lineage.md\nquarantined-at: 2026-09-01T18:00:00.000Z\nfault: unknown-key:status\n---\n",
  );
  assert.equal(quarantined.includes("blocked-by"), false);
  assert.equal(quarantined.includes("caused-by"), false);
  assert.equal(quarantined.includes("# Body"), false);
  assert.equal(/\nstatus:/.test(quarantined), false);
});

test("scan walks nested markdown under a configured folder", () => {
  const cwd = setup();
  mkdirSync(join(cwd, "notes", "sprints", "hivemind", "slices", "s-one"), {
    recursive: true,
  });
  writeFileSync(
    join(cwd, "notes", "sprints", "hivemind", "slices", "s-one", "spec.md"),
    "---\nid: s-one\n---\n",
  );
  writeFileSync(
    join(cwd, "notes", "sprints", "hivemind", "slices", "s-one", "oracles.md"),
    "# no front matter\n",
  );

  const result = scan({ cwd, now: NOW });

  assert.deepEqual(
    result.notes.map((note) => note.path),
    ["notes/sprints/hivemind/slices/s-one/spec.md"],
  );
  assert.equal(
    existsSync(
      join(
        cwd,
        "notes",
        "sprints",
        "hivemind",
        "slices",
        "s-one",
        "oracles.md",
      ),
    ),
    true,
  );
  assert.equal(existsSync(join(cwd, "quarantine", "oracles.md")), false);
});

test("scan creates the quarantine directory if missing before rename", () => {
  const cwd = withTemp("hivemind-scan-mkdir-");
  writeConfig(cwd, CONFIG);
  mkdirSync(join(cwd, "notes"));
  writeFileSync(join(cwd, "notes", "bad.md"), "---\n{\n---\n");

  scan({ cwd, now: NOW });

  assert.equal(existsSync(join(cwd, "notes", "bad.md")), false);
  assert.equal(
    readFileSync(join(cwd, "quarantine", "bad.md"), "utf8"),
    "---\norigin-location: notes/bad.md\nquarantined-at: 2026-09-01T18:00:00.000Z\nfault: parse-error\n---\n",
  );
});

test("watch paths are scanned instead of every folders path", () => {
  const cwd = withTemp("hivemind-scan-watch-");
  writeConfig(
    cwd,
    [
      "watch:",
      "  - notes/keep/**/*.md",
      "folders:",
      "  - path: notes/keep",
      "    schema:",
      "      id: string",
      "    required: [id]",
      "  - path: notes/skip",
      "    schema:",
      "      id: string",
      "    required: [id]",
      "  - path: quarantine",
      "    schema: quarantine",
      "    required: [origin-location, quarantined-at, fault]",
      "lanes: {}",
      "",
    ].join("\n"),
  );
  mkdirSync(join(cwd, "notes", "keep"), { recursive: true });
  mkdirSync(join(cwd, "notes", "skip"), { recursive: true });
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(join(cwd, "notes", "keep", "a.md"), "---\nid: a\n---\n");
  writeFileSync(join(cwd, "notes", "skip", "b.md"), "---\nid: b\n---\n");

  const result = scan({ cwd, now: NOW });
  assert.deepEqual(
    result.notes.map((note) => note.path),
    ["notes/keep/a.md"],
  );
});

test("unknown named folder schema fails closed", () => {
  const cwd = withTemp("hivemind-scan-unknown-named-");
  writeConfig(
    cwd,
    [
      "folders:",
      "  - path: tickets",
      "    schema: ticket",
      "    required: [id, status]",
      "  - path: quarantine",
      "    schema: quarantine",
      "    required: [origin-location, quarantined-at, fault]",
      "lanes: {}",
      "",
    ].join("\n"),
  );
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "quarantine"));

  assert.throws(() => scan({ cwd, now: NOW }), {
    message: 'Unknown folder schema "ticket"',
  });
});

test("inline folder schema keeps notes that use template extra keys", () => {
  const cwd = withTemp("hivemind-scan-allow-");
  writeConfig(
    cwd,
    [
      "folders:",
      "  - path: tickets",
      "    schema:",
      "      id: string",
      "      title: string",
      "      kind: string",
      "      status: string",
      "      labels: string",
      "      tags: string",
      "      sprint: string",
      "      slice: string",
      "      created_at: string",
      "      updated_at: string",
      "      claimed-by: string",
      "      caused-by: string",
      "      failed: string",
      "      intent: string",
      "    required: [id, status]",
      "  - path: planning",
      "    schema:",
      "      id: string",
      "      title: string",
      "      kind: string",
      "      status: string",
      "      sprint: string",
      "      tags: string",
      "      created_at: string",
      "      updated_at: string",
      "      claimed-by: string",
      "      blocked-by: string",
      "    required: [id, kind, status]",
      "  - path: quarantine",
      "    schema: quarantine",
      "    required: [origin-location, quarantined-at, fault]",
      "lanes: {}",
      "",
    ].join("\n"),
  );
  mkdirSync(join(cwd, "tickets"));
  mkdirSync(join(cwd, "planning", "sprints", "hivemind", "slices", "s-one"), {
    recursive: true,
  });
  mkdirSync(join(cwd, "quarantine"));
  writeFileSync(
    join(cwd, "tickets", "ticket-01.md"),
    [
      "---",
      "id: ticket-01",
      "title: example",
      "kind: ticket",
      "status: ready-for-agent",
      "labels: bug",
      "tags: []",
      "sprint: hivemind",
      "slice: s-one",
      "created_at: 2026-09-01T18:00:00Z",
      "updated_at: 2026-09-01T18:00:00Z",
      "claimed-by: run-1",
      "---",
      "",
    ].join("\n"),
  );
  writeFileSync(
    join(cwd, "planning", "sprints", "hivemind", "slices", "s-one", "spec.md"),
    [
      "---",
      "id: s-one",
      "title: one",
      "kind: slice",
      "status: ready",
      "sprint: hivemind",
      "tags: []",
      "created_at: 2026-09-01T18:00:00Z",
      "updated_at: 2026-09-01T18:00:00Z",
      "---",
      "",
    ].join("\n"),
  );

  const result = scan({ cwd, now: NOW });
  assert.deepEqual(result.notes.map((note) => note.path).sort(), [
    "planning/sprints/hivemind/slices/s-one/spec.md",
    "tickets/ticket-01.md",
  ]);
});
