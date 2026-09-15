import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import {
  createJournal,
  firstSpawnTimes,
  gcHistory,
  resolveHistory,
} from "../../src/journal/journal.ts";

const NOW = new Date("2026-09-02T11:54:26.000Z");

test("record writes a human line and a TSV row with header", () => {
  const cwd = withTemp("hivemind-journal-");
  const historyPath = join(cwd, "logs", "hivemind.tsv");
  const lines: string[] = [];
  const journal = createJournal({
    historyPath,
    writeLine: (line) => {
      lines.push(line);
    },
    now: () => NOW,
  });

  journal.record({
    kind: "claim",
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
  });

  assert.deepEqual(lines, ["hivemind claim plan tickets/agent.md"]);
  assert.equal(
    readFileSync(historyPath, "utf8"),
    [
      "ts\taction\tlane\tpath\trun_id\tdetail",
      "2026-09-02T11:54:26.000Z\tclaim\tplan\ttickets/agent.md\trun-1\t",
      "",
    ].join("\n"),
  );
});

test("second record appends without a second header", () => {
  const cwd = withTemp("hivemind-journal-append-");
  const historyPath = join(cwd, "hivemind.tsv");
  const journal = createJournal({
    historyPath,
    writeLine: () => {},
    now: () => NOW,
  });

  journal.record({
    kind: "scan",
    notes: 2,
    quarantined: 1,
  });
  journal.record({
    kind: "quarantine",
    path: "tickets/bad.md",
    fault: "parse-error",
  });

  assert.equal(
    readFileSync(historyPath, "utf8"),
    [
      "ts\taction\tlane\tpath\trun_id\tdetail",
      "2026-09-02T11:54:26.000Z\tscan\t\t\t\tnotes=2 quarantined=1",
      "2026-09-02T11:54:26.000Z\tquarantine\t\ttickets/bad.md\t\tparse-error",
      "",
    ].join("\n"),
  );
});

test("tabs and newlines in fields become spaces", () => {
  const cwd = withTemp("hivemind-journal-escape-");
  const historyPath = join(cwd, "hivemind.tsv");
  const journal = createJournal({
    historyPath,
    writeLine: () => {},
    now: () => NOW,
  });

  journal.record({
    kind: "quarantine",
    path: "tickets/bad.md",
    fault: "unknown-key:mystery\twith\nnewline",
  });

  const rows = readFileSync(historyPath, "utf8").trim().split("\n");
  assert.equal(rows.length, 2);
  assert.equal(rows[1]?.split("\t").length, 6);
  assert.equal(rows[1]?.includes("\twith"), false);
  assert.match(rows[1] ?? "", /unknown-key:mystery with newline/);
});

test("omitted history path still prints and does not write a file", () => {
  const cwd = withTemp("hivemind-journal-stderr-");
  const lines: string[] = [];
  const journal = createJournal({
    writeLine: (line) => {
      lines.push(line);
    },
    now: () => NOW,
  });

  journal.record({
    kind: "skip",
    lane: "plan",
    path: "tickets/agent.md",
    reason: "cmd-skip",
  });
  journal.record({
    kind: "spawn",
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
  });
  journal.record({
    kind: "exit",
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
    status: 0,
  });

  assert.deepEqual(lines, [
    "hivemind skip plan tickets/agent.md cmd-skip",
    "hivemind spawn plan tickets/agent.md",
    "hivemind exit plan tickets/agent.md status=0",
  ]);
  assert.equal(existsSync(join(cwd, "hivemind.tsv")), false);
});

test("spawn/exit carry stage and pid", () => {
  const cwd = withTemp("hivemind-journal-stage-pid-");
  const historyPath = join(cwd, "hivemind.tsv");
  const journal = createJournal({
    historyPath,
    writeLine: () => {},
    now: () => NOW,
  });

  journal.record({
    kind: "spawn",
    lane: "workflow",
    path: "tickets/agent.md",
    runId: "run-1",
    stage: "plan",
    pid: 8122,
  });
  journal.record({
    kind: "exit",
    lane: "workflow",
    path: "tickets/agent.md",
    runId: "run-1",
    status: 0,
    stage: "plan",
    pid: 8122,
  });

  assert.equal(
    readFileSync(historyPath, "utf8"),
    [
      "ts\taction\tlane\tpath\trun_id\tdetail",
      "2026-09-02T11:54:26.000Z\tspawn\tworkflow\ttickets/agent.md\trun-1\tstage=plan pid=8122",
      "2026-09-02T11:54:26.000Z\texit\tworkflow\ttickets/agent.md\trun-1\tstatus=0 stage=plan pid=8122",
      "",
    ].join("\n"),
  );
});

test("record revert writes a human line and a TSV row", () => {
  const cwd = withTemp("hivemind-journal-revert-");
  const historyPath = join(cwd, "hivemind.tsv");
  const lines: string[] = [];
  const journal = createJournal({
    historyPath,
    writeLine: (line) => {
      lines.push(line);
    },
    now: () => NOW,
  });

  journal.record({
    kind: "revert",
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
  });

  assert.deepEqual(lines, ["hivemind revert plan tickets/agent.md"]);
  assert.equal(
    readFileSync(historyPath, "utf8"),
    [
      "ts\taction\tlane\tpath\trun_id\tdetail",
      "2026-09-02T11:54:26.000Z\trevert\tplan\ttickets/agent.md\trun-1\t",
      "",
    ].join("\n"),
  );
});

test("resolveHistory joins relative paths and leaves empty unset", () => {
  assert.equal(resolveHistory({ cwd: "/tmp/p", path: undefined }), undefined);
  assert.equal(resolveHistory({ cwd: "/tmp/p", path: "" }), undefined);
  assert.equal(
    resolveHistory({ cwd: "/tmp/p", path: "logs/hivemind.tsv" }),
    join("/tmp/p", "logs/hivemind.tsv"),
  );
  assert.equal(
    resolveHistory({ cwd: "/tmp/p", path: "/abs/hivemind.tsv" }),
    "/abs/hivemind.tsv",
  );
});

test("hivemind.journal:fsync-repair: a torn tail does not drop a spawn timestamp", () => {
  const cwd = withTemp("hivemind-journal-torn-");
  const historyPath = join(cwd, "hivemind.tsv");
  const journal = createJournal({
    historyPath,
    writeLine: () => {},
    now: () => NOW,
  });
  journal.record({
    kind: "spawn",
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-1",
  });

  writeFileSync(
    historyPath,
    `${readFileSync(historyPath, "utf8")}2026-09-02T12:00:00.000Z\tspa`,
  );

  const skips: string[] = [];
  const origWrite = process.stderr.write.bind(process.stderr);
  process.stderr.write = ((chunk: string | Uint8Array, ...rest: unknown[]) => {
    skips.push(String(chunk));
    return origWrite(chunk, ...(rest as []));
  }) as typeof process.stderr.write;
  let times: Map<string, number>;
  try {
    times = firstSpawnTimes(historyPath);
  } finally {
    process.stderr.write = origWrite;
  }
  assert.equal(times.get("run-1"), Date.parse("2026-09-02T11:54:26.000Z"));
  assert.equal(readFileSync(historyPath, "utf8").endsWith("\n"), true);
  assert.equal(
    readFileSync(historyPath, "utf8").includes("2026-09-02T12:00:00.000Z\tspa"),
    false,
  );
  assert.match(skips.join(""), /hivemind skip torn-tail/);

  journal.record({
    kind: "claim",
    lane: "plan",
    path: "tickets/agent.md",
    runId: "run-2",
  });
  const rows = readFileSync(historyPath, "utf8").trim().split("\n");
  assert.equal(rows[1]?.split("\t").length, 6);
  assert.equal(rows[2]?.split("\t")[4], "run-2");
  assert.equal(
    firstSpawnTimes(historyPath).get("run-1"),
    Date.parse("2026-09-02T11:54:26.000Z"),
  );
});

test("hivemind.journal:gc: archives old history and firstSpawnTimes still loads", () => {
  const cwd = withTemp("hivemind-journal-gc-");
  const historyPath = join(cwd, "hivemind.tsv");
  writeFileSync(
    historyPath,
    [
      "ts\taction\tlane\tpath\trun_id\tdetail",
      "2026-01-01T00:00:00.000Z\tspawn\tplan\ttickets/old.md\trun-old\t",
      "2026-09-05T12:00:00.000Z\tspawn\tplan\ttickets/live.md\trun-live\t",
      "",
    ].join("\n"),
  );

  const result = gcHistory({
    historyPath,
    days: 30,
    now: new Date("2026-09-06T00:00:00.000Z"),
  });

  assert.equal(result.archived, 1);
  assert.equal(result.kept, 1);
  const live = readFileSync(historyPath, "utf8");
  assert.equal(live.includes("run-old"), false);
  assert.match(live, /run-live/);
  assert.equal(live.split("\n")[0], "ts\taction\tlane\tpath\trun_id\tdetail");
  const archive = readFileSync(`${historyPath}.archive`, "utf8");
  assert.match(archive, /run-old/);
  assert.equal(archive.includes("run-live"), false);
  const times = firstSpawnTimes(historyPath);
  assert.equal(times.get("run-live"), Date.parse("2026-09-05T12:00:00.000Z"));
  assert.equal(times.has("run-old"), false);
});
