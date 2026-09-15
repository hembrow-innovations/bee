import {
  closeSync,
  existsSync,
  fstatSync,
  fsyncSync,
  ftruncateSync,
  mkdirSync,
  openSync,
  readFileSync,
  readSync,
  renameSync,
  statSync,
  writeSync,
} from "node:fs";
import { dirname, isAbsolute, join } from "node:path";

export type HistoryEvent =
  | { kind: "scan"; notes: number; quarantined: number }
  | { kind: "quarantine"; path: string; fault: string }
  | { kind: "skip"; lane: string; path: string; reason: string }
  | { kind: "claim"; lane: string; path: string; runId: string }
  | {
      kind: "spawn";
      lane: string;
      path: string;
      runId: string;
      stage?: string;
      pid?: number;
    }
  | {
      kind: "exit";
      lane: string;
      path: string;
      runId: string;
      status: number;
      stage?: string;
      pid?: number;
      reason?: "timeout" | "stall";
    }
  | { kind: "revert"; lane: string; path: string; runId: string };

export type Journal = {
  record(event: HistoryEvent): void;
};

const HEADER_LINE = "ts\taction\tlane\tpath\trun_id\tdetail";
const HEADER = `${HEADER_LINE}\n`;
const TORN_SKIP = "hivemind skip torn-tail\n";
const DAY_MS = 86_400_000;

export function createJournal(opts: {
  historyPath?: string;
  writeLine?: (line: string) => void;
  now?: () => Date;
}): Journal {
  const writeLine =
    opts.writeLine ??
    ((line: string) => {
      process.stderr.write(`${line}\n`);
    });
  const now = opts.now ?? (() => new Date());
  return {
    record(event) {
      const ts = now().toISOString();
      writeLine(formatLine(event));
      if (opts.historyPath === undefined || opts.historyPath === "") return;
      writeRow({ path: opts.historyPath, ts, event });
    },
  };
}

export function resolveHistory(opts: {
  cwd: string;
  path: string | undefined;
}): string | undefined {
  if (opts.path === undefined || opts.path === "") return undefined;
  return isAbsolute(opts.path) ? opts.path : join(opts.cwd, opts.path);
}

export type HistoryRow = {
  ts: string;
  action: string;
  lane: string;
  path: string;
  runId: string;
  detail: string;
};

export function readHistory(historyPath: string | undefined): HistoryRow[] {
  if (historyPath === undefined || historyPath === "") return [];
  if (!existsSync(historyPath)) return [];
  repairTornTail(historyPath);
  const rows: HistoryRow[] = [];
  for (const line of readFileSync(historyPath, "utf8").split(/\r?\n/)) {
    if (line === "" || line.startsWith("ts\t")) continue;
    const cols = line.split("\t");
    if (cols.length < 6) continue;
    rows.push({
      ts: cols[0] ?? "",
      action: cols[1] ?? "",
      lane: cols[2] ?? "",
      path: cols[3] ?? "",
      runId: cols[4] ?? "",
      detail: cols[5] ?? "",
    });
  }
  return rows;
}

export function gcHistory(opts: {
  historyPath: string;
  days: number;
  now?: Date;
}): { archived: number; kept: number } {
  if (!Number.isInteger(opts.days) || opts.days < 0) {
    throw new Error("days must be a non-negative integer");
  }
  if (!existsSync(opts.historyPath)) {
    return { archived: 0, kept: 0 };
  }
  const cutoff = (opts.now ?? new Date()).getTime() - opts.days * DAY_MS;
  const old: HistoryRow[] = [];
  const keep: HistoryRow[] = [];
  for (const row of readHistory(opts.historyPath)) {
    const ms = Date.parse(row.ts);
    if (!Number.isNaN(ms) && ms < cutoff) old.push(row);
    else keep.push(row);
  }
  if (old.length === 0) return { archived: 0, kept: keep.length };
  const archivePath = `${opts.historyPath}.archive`;
  mkdirSync(dirname(archivePath), { recursive: true });
  if (!hasHeader(archivePath)) appendFsync(archivePath, HEADER);
  for (const row of old) appendFsync(archivePath, `${formatHistoryRow(row)}\n`);
  writeHistoryAtomic(opts.historyPath, keep);
  return { archived: old.length, kept: keep.length };
}

export function firstSpawnTimes(
  historyPath: string | undefined,
): Map<string, number> {
  const times = new Map<string, number>();
  for (const row of readHistory(historyPath)) {
    if (row.action !== "spawn") continue;
    if (row.runId === "") continue;
    if (times.has(row.runId)) continue;
    const ms = Date.parse(row.ts);
    if (Number.isNaN(ms)) continue;
    times.set(row.runId, ms);
  }
  return times;
}

function formatLine(event: HistoryEvent): string {
  switch (event.kind) {
    case "scan":
      return `hivemind scan notes=${event.notes} quarantined=${event.quarantined}`;
    case "quarantine":
      return `hivemind quarantine ${event.path} ${event.fault}`;
    case "skip":
      return `hivemind skip ${event.lane} ${event.path} ${event.reason}`;
    case "claim":
      return `hivemind claim ${event.lane} ${event.path}`;
    case "spawn":
      return `hivemind spawn ${event.lane} ${event.path}`;
    case "exit":
      return event.reason !== undefined
        ? `hivemind exit ${event.lane} ${event.path} status=${event.status} ${event.reason}`
        : `hivemind exit ${event.lane} ${event.path} status=${event.status}`;
    case "revert":
      return `hivemind revert ${event.lane} ${event.path}`;
    default: {
      const _exhaustive: never = event;
      return _exhaustive;
    }
  }
}

function writeRow(opts: {
  path: string;
  ts: string;
  event: HistoryEvent;
}): void {
  mkdirSync(dirname(opts.path), { recursive: true });
  repairTornTail(opts.path);
  if (!hasHeader(opts.path)) appendFsync(opts.path, HEADER);
  appendFsync(opts.path, `${tsvRow(opts.ts, opts.event)}\n`);
}

function repairTornTail(path: string): void {
  if (!existsSync(path)) return;
  const fd = openSync(path, "r+");
  try {
    const size = fstatSync(fd).size;
    if (size === 0) return;
    const last = Buffer.alloc(1);
    readSync(fd, last, 0, 1, size - 1);
    if (last[0] === 0x0a) return;
    const buf = Buffer.alloc(size);
    readSync(fd, buf, 0, size, 0);
    const content = buf.toString("utf8");
    const lastNl = content.lastIndexOf("\n");
    const tail = lastNl === -1 ? content : content.slice(lastNl + 1);
    if (isCompleteTail(tail)) {
      writeSync(fd, "\n", size);
      fsyncSync(fd);
      return;
    }
    ftruncateSync(fd, lastNl === -1 ? 0 : lastNl + 1);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  process.stderr.write(TORN_SKIP);
}

function isCompleteTail(tail: string): boolean {
  if (tail === HEADER_LINE) return true;
  return tail.split("\t").length === 6;
}

function appendFsync(path: string, data: string): void {
  const fd = openSync(path, "a");
  try {
    writeSync(fd, data);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

function writeHistoryAtomic(path: string, rows: HistoryRow[]): void {
  const tmp = `${path}.tmp`;
  const body = `${HEADER}${rows.map((row) => `${formatHistoryRow(row)}\n`).join("")}`;
  const fd = openSync(tmp, "w");
  try {
    writeSync(fd, body);
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  renameSync(tmp, path);
}

function formatHistoryRow(row: HistoryRow): string {
  return [row.ts, row.action, row.lane, row.path, row.runId, row.detail]
    .map(tsvField)
    .join("\t");
}

function hasHeader(path: string): boolean {
  if (!existsSync(path)) return false;
  return statSync(path).size > 0;
}

function tsvRow(ts: string, event: HistoryEvent): string {
  const fields = [ts, event.kind, "", "", "", ""];
  switch (event.kind) {
    case "scan":
      fields[5] = `notes=${event.notes} quarantined=${event.quarantined}`;
      break;
    case "quarantine":
      fields[3] = event.path;
      fields[5] = event.fault;
      break;
    case "skip":
      fields[2] = event.lane;
      fields[3] = event.path;
      fields[5] = event.reason;
      break;
    case "claim":
      fields[2] = event.lane;
      fields[3] = event.path;
      fields[4] = event.runId;
      break;
    case "spawn":
      fields[2] = event.lane;
      fields[3] = event.path;
      fields[4] = event.runId;
      fields[5] = spawnExitDetail(event);
      break;
    case "exit":
      fields[2] = event.lane;
      fields[3] = event.path;
      fields[4] = event.runId;
      fields[5] = spawnExitDetail(event);
      break;
    case "revert":
      fields[2] = event.lane;
      fields[3] = event.path;
      fields[4] = event.runId;
      break;
    default: {
      const _exhaustive: never = event;
      return _exhaustive;
    }
  }
  return fields.map(tsvField).join("\t");
}

function spawnExitDetail(
  event: Extract<HistoryEvent, { kind: "spawn" | "exit" }>,
): string {
  const parts: string[] = [];
  if (event.kind === "exit") parts.push(`status=${event.status}`);
  if (event.kind === "exit" && event.reason !== undefined) {
    parts.push(event.reason);
  }
  if (event.stage !== undefined && event.stage !== "") {
    parts.push(`stage=${event.stage}`);
  }
  if (event.pid !== undefined) parts.push(`pid=${event.pid}`);
  return parts.join(" ");
}

function tsvField(value: string): string {
  return value
    .replaceAll("\t", " ")
    .replaceAll("\r", " ")
    .replaceAll("\n", " ");
}
