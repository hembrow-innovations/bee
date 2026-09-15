import {
  mkdirSync,
  readFileSync,
  renameSync,
  rmdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, join } from "node:path";
import { parseYaml, type YamlValue } from "../yaml/yaml.ts";

export function parseFrontMatter(
  raw: string,
):
  | { kind: "ok"; map: Record<string, YamlValue> }
  | { kind: "fault"; fault: string } {
  if (!raw.startsWith("---")) {
    return { kind: "fault", fault: "parse-error" };
  }
  const afterOpen = raw.slice(3);
  const close = afterOpen.match(/\r?\n---(?:\r?\n|$)/);
  if (close === null) {
    return { kind: "fault", fault: "parse-error" };
  }
  const yamlText = afterOpen.slice(0, close.index).replace(/^\r?\n/, "");
  try {
    return { kind: "ok", map: parseYaml(yamlText) };
  } catch {
    return { kind: "fault", fault: "parse-error" };
  }
}

export function quarantineNote(opts: {
  abs: string;
  destDir: string;
  origin: string;
  fault: string;
  at: string;
}): void {
  mkdirSync(opts.destDir, { recursive: true });
  const dest = join(opts.destDir, basename(opts.abs));
  renameSync(opts.abs, dest);
  writeFileSync(
    dest,
    `---\norigin-location: ${opts.origin}\nquarantined-at: ${opts.at}\nfault: ${opts.fault}\n---\n`,
  );
}

export type ClaimResult = { kind: "claimed" } | { kind: "skipped" };
export type RevertResult = { kind: "reverted" } | { kind: "skipped" };

export function claim(opts: {
  abs: string;
  triggerStatus: YamlValue;
  claimStatus: string;
  runId: string;
  at?: string;
}): ClaimResult {
  const lockPath = `${opts.abs}.claimlock`;
  if (!tryLock(lockPath)) return { kind: "skipped" };
  try {
    const raw = readFileSync(opts.abs, "utf8");
    const parsed = parseFrontMatter(raw);
    if (parsed.kind === "fault") return { kind: "skipped" };
    if (!Object.is(parsed.map.status, opts.triggerStatus)) {
      return { kind: "skipped" };
    }
    writeFileSync(
      opts.abs,
      applyClaim(
        raw,
        opts.claimStatus,
        opts.runId,
        opts.at ?? new Date().toISOString(),
      ),
    );
    return { kind: "claimed" };
  } finally {
    rmdirSync(lockPath);
  }
}

export function revert(opts: {
  abs: string;
  claimStatus: string;
  triggerStatus: string;
  runId: string;
}): RevertResult {
  const lockPath = `${opts.abs}.claimlock`;
  if (!tryLock(lockPath)) return { kind: "skipped" };
  try {
    const raw = readFileSync(opts.abs, "utf8");
    const parsed = parseFrontMatter(raw);
    if (parsed.kind === "fault") return { kind: "skipped" };
    if (!Object.is(parsed.map.status, opts.claimStatus)) {
      return { kind: "skipped" };
    }
    if (!Object.is(parsed.map["claimed-by"], opts.runId)) {
      return { kind: "skipped" };
    }
    writeFileSync(opts.abs, applyRevert(raw, opts.triggerStatus));
    return { kind: "reverted" };
  } finally {
    rmdirSync(lockPath);
  }
}

const CLAIMLOCK_STALE_MS = 30_000;

function tryLock(lockPath: string): boolean {
  try {
    mkdirSync(lockPath);
    return true;
  } catch (err) {
    if (!isAlreadyExists(err)) throw err;
    if (!reapStaleLock(lockPath)) return false;
    try {
      mkdirSync(lockPath);
      return true;
    } catch (retryErr) {
      if (isAlreadyExists(retryErr)) return false;
      throw retryErr;
    }
  }
}

function reapStaleLock(lockPath: string): boolean {
  try {
    const ageMs = Date.now() - statSync(lockPath).mtimeMs;
    if (ageMs < CLAIMLOCK_STALE_MS) return false;
    rmdirSync(lockPath);
    return true;
  } catch (err) {
    return isNotFound(err);
  }
}

function isAlreadyExists(err: unknown): boolean {
  return isFsCode(err, "EEXIST");
}

function isNotFound(err: unknown): boolean {
  return isFsCode(err, "ENOENT");
}

function isFsCode(err: unknown, code: string): boolean {
  return (
    typeof err === "object" &&
    err !== null &&
    "code" in err &&
    err.code === code
  );
}

function applyClaim(
  raw: string,
  claimStatus: string,
  runId: string,
  claimedAt: string,
): string {
  const match = raw.match(/^(---\r?\n)([\s\S]*?)(\r?\n---(?:\r?\n|$))/);
  if (
    match === null ||
    match[1] === undefined ||
    match[2] === undefined ||
    match[3] === undefined
  ) {
    return raw;
  }
  let front = match[2];
  front = upsertKey(front, "status", claimStatus);
  front = upsertKey(front, "claimed-by", runId);
  front = upsertKey(front, "claimed-at", claimedAt);
  return `${match[1]}${front}${match[3]}${raw.slice(match[0].length)}`;
}

function applyRevert(raw: string, triggerStatus: string): string {
  const match = raw.match(/^(---\r?\n)([\s\S]*?)(\r?\n---(?:\r?\n|$))/);
  if (
    match === null ||
    match[1] === undefined ||
    match[2] === undefined ||
    match[3] === undefined
  ) {
    return raw;
  }
  let front = match[2];
  front = upsertKey(front, "status", triggerStatus);
  front = deleteKey(front, "claimed-by");
  front = deleteKey(front, "claimed-at");
  return `${match[1]}${front}${match[3]}${raw.slice(match[0].length)}`;
}

function upsertKey(front: string, key: string, value: string): string {
  const line = `${key}: ${value}`;
  const pattern = new RegExp(`^${key}:.*$`, "m");
  if (pattern.test(front)) return front.replace(pattern, line);
  if (front === "") return line;
  return `${front}\n${line}`;
}

function deleteKey(front: string, key: string): string {
  const pattern = new RegExp(`^${key}:.*(?:\r?\n)?`, "m");
  return front.replace(pattern, "").replace(/\n+$/, "").replace(/^\n+/, "");
}
