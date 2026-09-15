#!/usr/bin/env node
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { runOnce } from "./loop/once.ts";
import { runDryRun, runExplain, runGc, runStatus } from "./loop/operator.ts";
import { runWatch } from "./loop/watch.ts";
import type { SpawnChild } from "./loop/matches.ts";

export type { SpawnChild };

export async function run(opts: {
  argv: readonly string[];
  cwd: string;
  spawnChild?: SpawnChild;
  signal?: AbortSignal;
  env?: NodeJS.ProcessEnv;
}): Promise<number> {
  const command = opts.argv[0];
  if (
    command === undefined ||
    command === "-h" ||
    command === "--help" ||
    command === "help"
  ) {
    usage();
    return 0;
  }
  if (command === "status") {
    try {
      runStatus({ cwd: opts.cwd });
      return 0;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      console.error(message);
      return 1;
    }
  }
  if (command === "gc") {
    try {
      const flags = parseGcFlags(opts.argv.slice(1));
      runGc({ cwd: opts.cwd, days: flags.days });
      return 0;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      console.error(message);
      return 1;
    }
  }
  if (command === "explain") {
    const path = opts.argv[1];
    if (path === undefined || path === "") {
      console.error("explain requires a path");
      return 1;
    }
    try {
      runExplain({ cwd: opts.cwd, path });
      return 0;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      console.error(message);
      return 1;
    }
  }
  if (command !== "once" && command !== "watch") {
    console.error(`Unknown command: ${command}`);
    return 1;
  }
  try {
    if (command === "watch") {
      const flags = parseWatchFlags(opts.argv.slice(1));
      await runWatch({
        cwd: opts.cwd,
        untilQuiet: flags.untilQuiet,
        untilTarget: flags.untilTarget,
        maxSpawns: flags.maxSpawns,
        spawnChild: opts.spawnChild,
        signal: opts.signal,
      });
    } else {
      const flags = parseOnceFlags(opts.argv.slice(1));
      if (flags.dryRun) {
        runDryRun({ cwd: opts.cwd, env: opts.env });
      } else {
        await runOnce({
          cwd: opts.cwd,
          spawnChild: opts.spawnChild,
          env: opts.env,
        });
      }
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    console.error(message);
    return 1;
  }
  return 0;
}

function parseGcFlags(argv: readonly string[]): { days: number } {
  let days: number | undefined;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--days") {
      const raw = argv[i + 1];
      if (raw === undefined || raw === "") {
        throw new Error("--days requires a count");
      }
      const n = Number(raw);
      if (!Number.isInteger(n) || n < 0) {
        throw new Error("--days requires a non-negative integer");
      }
      days = n;
      i += 1;
      continue;
    }
    throw new Error(`Unknown flag: ${arg}`);
  }
  if (days === undefined) {
    throw new Error("--days requires a count");
  }
  return { days };
}

function parseOnceFlags(argv: readonly string[]): { dryRun: boolean } {
  let dryRun = false;
  for (const arg of argv) {
    if (arg === "--dry-run") {
      dryRun = true;
      continue;
    }
    throw new Error(`Unknown flag: ${arg}`);
  }
  return { dryRun };
}

function parseWatchFlags(argv: readonly string[]): {
  untilQuiet: boolean;
  untilTarget: string | undefined;
  maxSpawns: number | undefined;
} {
  let untilQuiet = false;
  let untilTarget: string | undefined;
  let maxSpawns: number | undefined;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--until-quiet") {
      untilQuiet = true;
      continue;
    }
    if (arg === "--until-target") {
      const path = argv[i + 1];
      if (path === undefined || path === "") {
        throw new Error("--until-target requires a path");
      }
      untilTarget = path;
      i += 1;
      continue;
    }
    if (arg === "--max-spawns") {
      const raw = argv[i + 1];
      if (raw === undefined || raw === "") {
        throw new Error("--max-spawns requires a count");
      }
      const n = Number(raw);
      if (!Number.isInteger(n) || n < 0) {
        throw new Error("--max-spawns requires a non-negative integer");
      }
      maxSpawns = n;
      i += 1;
      continue;
    }
    throw new Error(`Unknown flag: ${arg}`);
  }
  return { untilQuiet, untilTarget, maxSpawns };
}

function usage(): void {
  console.log(`hivemind

Usage:
  node --experimental-strip-types src/cli.ts <command>

Commands:
  once              one scan, spawn matches, wait, exit
  watch             resident predicate loop
  status            last scan, live runs, skip histogram, quarantined, ttl-expired
  explain <path>    match, need miss, exclusive skip, last skip
  gc                archive history rows older than --days N

Options:
  -h, --help              Show help
  --dry-run               once: print plan with secrets redacted, no claim, no spawn
  --until-quiet           watch: exit after one quiet scan
  --until-target PATH     watch: exit when PATH exists
  --max-spawns N          watch: stop new claims after N spawns
  --days N                gc: archive history rows older than N days

Events print to stderr. Optional history TSV is set in .hivemind/hivemind.yaml.
`);
}

const entry = process.argv[1];
if (entry !== undefined && fileURLToPath(import.meta.url) === resolve(entry)) {
  const ac = new AbortController();
  const onStop = () => ac.abort();
  process.on("SIGTERM", onStop);
  process.on("SIGINT", onStop);
  const status = await run({
    argv: process.argv.slice(2),
    cwd: process.cwd(),
    signal: ac.signal,
  });
  process.off("SIGTERM", onStop);
  process.off("SIGINT", onStop);
  process.exit(status);
}
