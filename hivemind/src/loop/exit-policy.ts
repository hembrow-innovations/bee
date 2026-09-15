import { spawnSync } from "node:child_process";
import type { Journal } from "../journal/journal.ts";

export function normalizePrefix(path: string): string {
  return path.replaceAll("\\", "/").replace(/\/+$/, "");
}

export function applyExitPolicy(opts: {
  audit?: "git-diff";
  prefixes: readonly string[];
  cwd: string;
  env: NodeJS.ProcessEnv;
  journal?: Journal;
  lane: string;
  path: string;
}): void {
  if (opts.audit !== "git-diff") return;
  const names = diffNames(opts.cwd, opts.env);
  if (names === undefined || names.length === 0) return;
  if (names.some((name) => !inScope(name, opts.prefixes))) {
    opts.journal?.record({
      kind: "skip",
      lane: opts.lane,
      path: opts.path,
      reason: "scope-breach",
    });
  }
}

function inScope(name: string, prefixes: readonly string[]): boolean {
  const n = normalizePrefix(name);
  return prefixes.some((prefix) => {
    const p = normalizePrefix(prefix);
    return n === p || n.startsWith(`${p}/`);
  });
}

function diffNames(cwd: string, env: NodeJS.ProcessEnv): string[] | undefined {
  try {
    const result = spawnSync("git", ["diff", "--name-only"], {
      cwd,
      env: { ...process.env, ...env },
      encoding: "utf8",
    });
    if (result.error !== undefined) return undefined;
    if (result.status !== 0) return undefined;
    const out = result.stdout ?? "";
    return out.split(/\r?\n/).filter((name) => name !== "");
  } catch {
    return undefined;
  }
}
