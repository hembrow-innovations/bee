import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const dirs: string[] = [];

function rm(dir: string): void {
  try {
    rmSync(dir, { recursive: true, force: true });
  } catch {
    return;
  }
}

export function cleanupTemp(dir?: string): void {
  if (dir) {
    const i = dirs.indexOf(dir);
    if (i >= 0) dirs.splice(i, 1);
    rm(dir);
    return;
  }
  for (const leftover of dirs.splice(0)) rm(leftover);
}

export function withTemp(prefix: string): string;
export function withTemp<T>(prefix: string, fn: (dir: string) => T): T;
export function withTemp<T>(
  prefix: string,
  fn?: (dir: string) => T,
): string | T {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  if (!fn) {
    dirs.push(dir);
    return dir;
  }
  try {
    const result = fn(dir);
    if (result && typeof result === "object" && "then" in result) {
      return (result as Promise<unknown>).finally(() => rm(dir)) as T;
    }
    rm(dir);
    return result;
  } catch (err) {
    rm(dir);
    throw err;
  }
}

process.on("exit", cleanupTemp);
