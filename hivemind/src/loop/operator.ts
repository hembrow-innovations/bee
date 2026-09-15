import { loadConfig, type Lane, type SpawnSpec } from "../config/loadConfig.ts";
import {
  gcHistory,
  readHistory,
  resolveHistory,
  type HistoryRow,
} from "../journal/journal.ts";
import { matchNotes } from "../match/matcher.ts";
import { scan, type ScannedNote } from "../scan/scan.ts";
import { renderArgv } from "../spawn/spawner.ts";
import { exclusiveSetsOverlap } from "./matches.ts";

export function runStatus(opts: { cwd: string; now?: number }): void {
  for (const line of statusLines(opts)) console.log(line);
}

export function runExplain(opts: { cwd: string; path: string }): void {
  for (const line of explainLines(opts)) console.log(line);
}

export function runDryRun(opts: {
  cwd: string;
  env?: NodeJS.ProcessEnv;
}): void {
  for (const line of dryRunLines(opts)) console.log(line);
}

export function runGc(opts: { cwd: string; days: number; now?: Date }): void {
  const config = loadConfig(opts.cwd);
  const historyPath = resolveHistory({ cwd: opts.cwd, path: config.history });
  if (historyPath === undefined) {
    throw new Error("gc requires history");
  }
  const result = gcHistory({
    historyPath,
    days: opts.days,
    now: opts.now,
  });
  console.log(`gc archived=${result.archived} kept=${result.kept}`);
}

function explainLines(opts: { cwd: string; path: string }): string[] {
  const config = loadConfig(opts.cwd);
  const rows = readHistory(
    resolveHistory({ cwd: opts.cwd, path: config.history }),
  );
  const lanes = config.lanes.filter(
    (lane) => !config.disable.includes(lane.lane),
  );
  const { notes } = scan({ cwd: opts.cwd, config, readonly: true });
  const live = liveRuns(rows);
  const lines: string[] = [];
  for (const lane of lanes) {
    const verdict = explainLane({
      lane,
      path: opts.path,
      notes,
      lanes,
      live,
      cwd: opts.cwd,
    });
    if (verdict === undefined) continue;
    lines.push(`${verdict} ${lane.lane}`);
  }
  const lastSkip = lastSkipReason(rows, opts.path);
  lines.push(`last-skip ${lastSkip ?? "none"}`);
  return lines;
}

function explainLane(opts: {
  lane: Lane;
  path: string;
  notes: readonly ScannedNote[];
  lanes: readonly Lane[];
  live: readonly HistoryRow[];
  cwd: string;
}): "match" | "need" | "exclusive" | undefined {
  const note = opts.notes.filter((item) => item.path === opts.path);
  if (note.length === 0) return undefined;
  const triggerHit = matchNotes({
    lanes: [withoutNeed(opts.lane)],
    notes: opts.notes,
    cwd: opts.cwd,
  }).some((item) => item.note.path === opts.path);
  if (!triggerHit) return undefined;
  const fullHit = matchNotes({
    lanes: [opts.lane],
    notes: opts.notes,
    cwd: opts.cwd,
  }).some((item) => item.note.path === opts.path);
  if (!fullHit) return "need";
  if (exclusiveCollides(opts.lane, opts.lanes, opts.live)) return "exclusive";
  return "match";
}

function withoutNeed(lane: Lane): Lane {
  return { ...lane, need: undefined };
}

function exclusiveCollides(
  lane: Lane,
  lanes: readonly Lane[],
  live: readonly HistoryRow[],
): boolean {
  const byId = new Map(lanes.map((item) => [item.lane, item]));
  for (const run of live) {
    const other = byId.get(run.lane);
    if (other === undefined) continue;
    if (exclusiveSetsOverlap(lane.exclusive, other.exclusive)) return true;
  }
  return false;
}

function lastSkipReason(
  rows: readonly HistoryRow[],
  path: string,
): string | undefined {
  for (let i = rows.length - 1; i >= 0; i -= 1) {
    const row = rows[i];
    if (row?.action !== "skip") continue;
    if (row.path !== path) continue;
    if (row.detail === "") return undefined;
    return row.detail;
  }
  return undefined;
}

function statusLines(opts: { cwd: string; now?: number }): string[] {
  const config = loadConfig(opts.cwd);
  const rows = readHistory(
    resolveHistory({ cwd: opts.cwd, path: config.history }),
  );
  const lines: string[] = [];
  const lastScan = lastRow(rows, "scan");
  if (lastScan === undefined) {
    lines.push("last-scan none");
  } else {
    const notes = detailField(lastScan.detail, "notes") ?? "0";
    const quarantined = detailField(lastScan.detail, "quarantined") ?? "0";
    lines.push(`last-scan notes=${notes} quarantined=${quarantined}`);
  }
  const now = opts.now ?? Date.now();
  for (const run of liveRuns(rows)) {
    const age = ageSeconds(run.ts, now);
    const pid = detailField(run.detail, "pid") ?? "-";
    lines.push(
      `live lane=${run.lane} path=${run.path} pid=${pid} run-id=${run.runId} age=${age}s`,
    );
  }
  const skips = skipHistogram(rows);
  if (skips.length > 0) {
    const parts = skips.map(([reason, count]) => `${reason}=${count}`);
    lines.push(`skip ${parts.join(" ")}`);
  }
  const quarantined = detailField(lastScan?.detail ?? "", "quarantined") ?? "0";
  lines.push(`quarantined ${quarantined}`);
  const expired = rows.filter((row) => row.action === "revert").length;
  lines.push(`ttl-expired ${expired}`);
  return lines;
}

function liveRuns(rows: readonly HistoryRow[]): HistoryRow[] {
  const live = new Map<string, HistoryRow>();
  for (const row of rows) {
    if (row.runId === "") continue;
    if (row.action === "spawn") live.set(row.runId, row);
    if (row.action === "exit") live.delete(row.runId);
  }
  return [...live.values()];
}

function skipHistogram(rows: readonly HistoryRow[]): [string, number][] {
  const counts = new Map<string, number>();
  const order: string[] = [];
  for (const row of rows) {
    if (row.action !== "skip") continue;
    const reason = row.detail;
    if (reason === "") continue;
    const current = counts.get(reason);
    if (current === undefined) {
      order.push(reason);
      counts.set(reason, 1);
      continue;
    }
    counts.set(reason, current + 1);
  }
  return order.map((reason) => [reason, counts.get(reason) ?? 0]);
}

function lastRow(
  rows: readonly HistoryRow[],
  action: string,
): HistoryRow | undefined {
  for (let i = rows.length - 1; i >= 0; i -= 1) {
    const row = rows[i];
    if (row?.action === action) return row;
  }
  return undefined;
}

function detailField(detail: string, key: string): string | undefined {
  const prefix = `${key}=`;
  for (const part of detail.split(" ")) {
    if (part.startsWith(prefix)) return part.slice(prefix.length);
  }
  return undefined;
}

function ageSeconds(ts: string, now: number): number {
  const ms = Date.parse(ts);
  if (Number.isNaN(ms)) return 0;
  return Math.max(0, Math.floor((now - ms) / 1000));
}

function dryRunLines(opts: { cwd: string; env?: NodeJS.ProcessEnv }): string[] {
  const config = loadConfig(opts.cwd);
  const lanes = config.lanes.filter(
    (lane) => !config.disable.includes(lane.lane),
  );
  if (lanes.length === 0) return [];
  const { notes } = scan({ cwd: opts.cwd, config, readonly: true });
  const matches = matchNotes({ lanes, notes, cwd: opts.cwd });
  const env = opts.env ?? process.env;
  const plannedByLane = new Map<string, number>();
  const lines: string[] = [];
  for (const match of matches) {
    const used = plannedByLane.get(match.lane.lane) ?? 0;
    if (used >= match.lane.concurrency) continue;
    const rendered = renderArgv({
      specs: spawnSpecs(match.lane),
      lane: match.lane.lane,
      cwd: opts.cwd,
      env,
      runId: "dry-run",
      path: match.note.path,
      stages: spawnStages(match.lane),
      redactEnv: true,
    });
    if (rendered.kind === "skip") continue;
    plannedByLane.set(match.lane.lane, used + 1);
    for (const argv of rendered.argvList) {
      lines.push(
        `plan lane=${match.lane.lane} path=${match.note.path} argv=${argv.join(" ")}`,
      );
    }
  }
  return lines;
}

function spawnSpecs(lane: Lane): SpawnSpec[] {
  if (lane.type === "single") return [lane];
  return [...lane.stages];
}

function spawnStages(lane: Lane): readonly string[] | undefined {
  if (lane.type === "single") return undefined;
  return lane.stages.map((stage) => stage.stage);
}
