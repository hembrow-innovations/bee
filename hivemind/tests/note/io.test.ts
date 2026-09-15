import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import {
  claim,
  parseFrontMatter,
  quarantineNote,
  revert,
} from "../../src/note/io.ts";

test("parseFrontMatter returns the yaml map inside the fence", () => {
  const parsed = parseFrontMatter(
    "---\nid: good\nstatus: ready-for-agent\n---\n\n# body\n",
  );
  assert.deepEqual(parsed, {
    kind: "ok",
    map: { id: "good", status: "ready-for-agent" },
  });
});

test("parseFrontMatter faults on an unclosed fence or illegal yaml", () => {
  assert.deepEqual(parseFrontMatter("---\nid: dangling\n"), {
    kind: "fault",
    fault: "parse-error",
  });
  assert.deepEqual(parseFrontMatter("---\n{\n---\n"), {
    kind: "fault",
    fault: "parse-error",
  });
});

test("quarantineNote moves the file and writes only origin-location, quarantined-at, fault", () => {
  const cwd = withTemp("hivemind-note-");
  const notes = join(cwd, "notes");
  const destDir = join(cwd, "quarantine");
  mkdirSync(notes);
  mkdirSync(destDir);
  const abs = join(notes, "bad.md");
  writeFileSync(
    abs,
    [
      "---",
      "id: lineage",
      "status: ready-for-agent",
      "blocked-by: other",
      "caused-by: parent",
      "---",
      "",
      "# Body the supervisor must not copy",
      "",
    ].join("\n"),
  );

  quarantineNote({
    abs,
    destDir,
    origin: "notes/bad.md",
    fault: "unknown-key:status",
    at: "2026-09-01T18:00:00.000Z",
  });

  assert.equal(existsSync(abs), false);
  const quarantined = readFileSync(join(destDir, "bad.md"), "utf8");
  assert.equal(
    quarantined,
    "---\norigin-location: notes/bad.md\nquarantined-at: 2026-09-01T18:00:00.000Z\nfault: unknown-key:status\n---\n",
  );
});

test("quarantineNote creates the dest directory if missing", () => {
  const cwd = withTemp("hivemind-note-mkdir-");
  const notes = join(cwd, "notes");
  mkdirSync(notes);
  const abs = join(notes, "bad.md");
  writeFileSync(abs, "---\nid: bad\n---\n");

  quarantineNote({
    abs,
    destDir: join(cwd, "quarantine"),
    origin: "notes/bad.md",
    fault: "parse-error",
    at: "2026-09-01T18:00:00.000Z",
  });

  assert.equal(existsSync(abs), false);
  assert.equal(
    readFileSync(join(cwd, "quarantine", "bad.md"), "utf8"),
    "---\norigin-location: notes/bad.md\nquarantined-at: 2026-09-01T18:00:00.000Z\nfault: parse-error\n---\n",
  );
});

test("claim writes claim-status and claimed-by and keeps the body", () => {
  const cwd = withTemp("hivemind-note-claim-");
  const abs = join(cwd, "ticket.md");
  writeFileSync(
    abs,
    "---\nid: t1\nstatus: ready-for-agent\n---\n\n# keep this body\n",
  );

  const result = claim({
    abs,
    triggerStatus: "ready-for-agent",
    claimStatus: "active",
    runId: "run-1",
    at: "2026-09-05T20:45:00.000Z",
  });

  assert.deepEqual(result, { kind: "claimed" });
  assert.equal(
    readFileSync(abs, "utf8"),
    "---\nid: t1\nstatus: active\nclaimed-by: run-1\nclaimed-at: 2026-09-05T20:45:00.000Z\n---\n\n# keep this body\n",
  );
  assert.equal(existsSync(`${abs}.claimlock`), false);
});

test("claim skips when status no longer matches the trigger", () => {
  const cwd = withTemp("hivemind-note-status-");
  const abs = join(cwd, "ticket.md");
  const raw = "---\nid: t1\nstatus: active\n---\n\n# body\n";
  writeFileSync(abs, raw);

  const result = claim({
    abs,
    triggerStatus: "ready-for-agent",
    claimStatus: "active",
    runId: "run-1",
  });

  assert.deepEqual(result, { kind: "skipped" });
  assert.equal(readFileSync(abs, "utf8"), raw);
  assert.equal(existsSync(`${abs}.claimlock`), false);
});

test("claim skips when the claim lock already exists", () => {
  const cwd = withTemp("hivemind-note-lock-");
  const abs = join(cwd, "ticket.md");
  const raw = "---\nid: t1\nstatus: ready-for-agent\n---\n\n# body\n";
  writeFileSync(abs, raw);
  mkdirSync(`${abs}.claimlock`);

  const result = claim({
    abs,
    triggerStatus: "ready-for-agent",
    claimStatus: "active",
    runId: "run-1",
  });

  assert.deepEqual(result, { kind: "skipped" });
  assert.equal(readFileSync(abs, "utf8"), raw);
  assert.equal(existsSync(`${abs}.claimlock`), true);
});

test("hivemind.note:claimlock-stale: a claimlock older than the lease age is removed and claim retries", () => {
  const cwd = withTemp("hivemind-note-stale-lock-");
  const abs = join(cwd, "ticket.md");
  writeFileSync(
    abs,
    "---\nid: t1\nstatus: ready-for-agent\n---\n\n# keep this body\n",
  );
  const lockPath = `${abs}.claimlock`;
  mkdirSync(lockPath);
  const stale = new Date(Date.now() - 120_000);
  utimesSync(lockPath, stale, stale);

  const result = claim({
    abs,
    triggerStatus: "ready-for-agent",
    claimStatus: "active",
    runId: "run-1",
    at: "2026-09-05T20:45:00.000Z",
  });

  assert.deepEqual(result, { kind: "claimed" });
  assert.equal(
    readFileSync(abs, "utf8"),
    "---\nid: t1\nstatus: active\nclaimed-by: run-1\nclaimed-at: 2026-09-05T20:45:00.000Z\n---\n\n# keep this body\n",
  );
  assert.equal(existsSync(lockPath), false);
});

test("claim skips when the file is not parseable front matter", () => {
  const cwd = withTemp("hivemind-note-fault-");
  const abs = join(cwd, "ticket.md");
  const raw = "---\n{\n---\n";
  writeFileSync(abs, raw);

  const result = claim({
    abs,
    triggerStatus: "ready-for-agent",
    claimStatus: "active",
    runId: "run-1",
  });

  assert.deepEqual(result, { kind: "skipped" });
  assert.equal(readFileSync(abs, "utf8"), raw);
  assert.equal(existsSync(`${abs}.claimlock`), false);
});

test("revert writes trigger status, deletes claimed-by, and keeps the body", () => {
  const cwd = withTemp("hivemind-note-revert-");
  const abs = join(cwd, "ticket.md");
  writeFileSync(
    abs,
    "---\nid: t1\nstatus: active\nclaimed-by: run-1\nclaimed-at: 2026-09-05T20:45:00.000Z\n---\n\n# keep this body\n",
  );

  const result = revert({
    abs,
    claimStatus: "active",
    triggerStatus: "ready-for-agent",
    runId: "run-1",
  });

  assert.deepEqual(result, { kind: "reverted" });
  assert.equal(
    readFileSync(abs, "utf8"),
    "---\nid: t1\nstatus: ready-for-agent\n---\n\n# keep this body\n",
  );
  assert.equal(existsSync(`${abs}.claimlock`), false);
});

test("hivemind.note:claimlock-stale: a claimlock older than the lease age is removed and revert retries", () => {
  const cwd = withTemp("hivemind-note-stale-revert-lock-");
  const abs = join(cwd, "ticket.md");
  writeFileSync(
    abs,
    "---\nid: t1\nstatus: active\nclaimed-by: run-1\n---\n\n# keep this body\n",
  );
  const lockPath = `${abs}.claimlock`;
  mkdirSync(lockPath);
  const stale = new Date(Date.now() - 120_000);
  utimesSync(lockPath, stale, stale);

  const result = revert({
    abs,
    claimStatus: "active",
    triggerStatus: "ready-for-agent",
    runId: "run-1",
  });

  assert.deepEqual(result, { kind: "reverted" });
  assert.equal(
    readFileSync(abs, "utf8"),
    "---\nid: t1\nstatus: ready-for-agent\n---\n\n# keep this body\n",
  );
  assert.equal(existsSync(lockPath), false);
});
