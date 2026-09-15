import { loadConfig } from "../config/loadConfig.ts";
import { createJournal, resolveHistory } from "../journal/journal.ts";
import type { LiveRun, SpawnChild } from "./matches.ts";
import { runTick } from "./tick.ts";

export type { LiveRun, SpawnChild };

export async function runOnce(opts: {
  cwd: string;
  spawnChild?: SpawnChild;
  env?: NodeJS.ProcessEnv;
}): Promise<void> {
  const config = loadConfig(opts.cwd);
  const journal = createJournal({
    historyPath: resolveHistory({ cwd: opts.cwd, path: config.history }),
  });
  const lanes = config.lanes.filter(
    (lane) => !config.disable.includes(lane.lane),
  );
  if (lanes.length === 0) {
    journal.record({ kind: "scan", notes: 0, quarantined: 0 });
    return;
  }
  const env = opts.env ?? process.env;
  const live: LiveRun[] = [];
  runTick({
    cwd: opts.cwd,
    config,
    lanes,
    env,
    spawnChild: opts.spawnChild,
    live,
    journal,
  });
  await Promise.all(live.map((run) => run.wait));
}
