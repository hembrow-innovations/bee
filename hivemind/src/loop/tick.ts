import { join } from "node:path";
import type { HivemindConfig, Lane } from "../config/loadConfig.ts";
import {
  firstSpawnTimes,
  resolveHistory,
  type Journal,
} from "../journal/journal.ts";
import { matchNotes } from "../match/matcher.ts";
import { revert } from "../note/io.ts";
import { scan, type ScannedNote } from "../scan/scan.ts";
import {
  reapDeadRuns,
  spawnMatches,
  type LiveRun,
  type SpawnChild,
} from "./matches.ts";

export function runTick(opts: {
  cwd: string;
  config: HivemindConfig;
  lanes: readonly Lane[];
  env: NodeJS.ProcessEnv;
  spawnChild?: SpawnChild;
  live: LiveRun[];
  journal: Journal;
  lastFinished?: Map<string, number>;
  spawnBudget?: number;
}): number {
  const { notes, quarantines } = scan({ cwd: opts.cwd, config: opts.config });
  opts.journal.record({
    kind: "scan",
    notes: notes.length,
    quarantined: quarantines.length,
  });
  for (const item of quarantines) {
    opts.journal.record({
      kind: "quarantine",
      path: item.path,
      fault: item.fault,
    });
  }
  revertStaleClaims({
    cwd: opts.cwd,
    config: opts.config,
    lanes: opts.lanes,
    notes,
    live: opts.live,
    journal: opts.journal,
  });
  const matches = matchNotes({
    lanes: opts.lanes,
    notes,
    disable: opts.config.disable,
    cwd: opts.cwd,
    journal: opts.journal,
  });
  return spawnMatches({
    cwd: opts.cwd,
    matches,
    env: opts.env,
    spawnChild: opts.spawnChild,
    live: opts.live,
    journal: opts.journal,
    lastFinished: opts.lastFinished,
    spawnBudget: opts.spawnBudget,
  });
}

function revertStaleClaims(opts: {
  cwd: string;
  config: HivemindConfig;
  lanes: readonly Lane[];
  notes: readonly ScannedNote[];
  live: readonly LiveRun[];
  journal: Journal;
}): void {
  const ttlLanes = opts.lanes.filter((lane) => lane.ttlMs > 0);
  if (ttlLanes.length === 0) return;
  const spawnedAt = firstSpawnTimes(
    resolveHistory({ cwd: opts.cwd, path: opts.config.history }),
  );
  reapDeadRuns(opts.live);
  const livePaths = new Set(
    opts.live.filter((run) => !run.done).map((run) => run.path),
  );
  const now = Date.now();
  for (const note of opts.notes) {
    if (livePaths.has(note.path)) continue;
    const claimedBy = note.frontMatter["claimed-by"];
    if (typeof claimedBy !== "string" || claimedBy === "") continue;
    const spawnAt = spawnedAt.get(claimedBy);
    const claimedAt = parseClaimedAt(note.frontMatter["claimed-at"]);
    const startedAt = spawnAt ?? claimedAt;
    if (startedAt === undefined) continue;
    for (const lane of ttlLanes) {
      if (!Object.is(note.frontMatter.status, lane.claimStatus)) continue;
      if (now - startedAt < lane.ttlMs) continue;
      const triggerStatus = lane.trigger.status;
      if (typeof triggerStatus !== "string") continue;
      const taken = revert({
        abs: join(opts.cwd, note.path),
        claimStatus: lane.claimStatus,
        triggerStatus,
        runId: claimedBy,
      });
      if (taken.kind !== "reverted") continue;
      opts.journal.record({
        kind: "revert",
        lane: lane.lane,
        path: note.path,
        runId: claimedBy,
      });
      break;
    }
  }
}

function parseClaimedAt(value: unknown): number | undefined {
  if (typeof value !== "string" || value === "") return undefined;
  const ms = Date.parse(value);
  if (Number.isNaN(ms)) return undefined;
  return ms;
}
