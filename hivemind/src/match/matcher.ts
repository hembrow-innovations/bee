import { existsSync } from "node:fs";
import { join } from "node:path";
import type { Lane } from "../config/loadConfig.ts";
import type { Journal } from "../journal/journal.ts";
import type { ScannedNote } from "../scan/scan.ts";
import type { YamlValue } from "../yaml/yaml.ts";

export type Match = {
  lane: Lane;
  note: ScannedNote;
};

export function matchNotes(opts: {
  lanes: readonly Lane[];
  notes: readonly ScannedNote[];
  disable?: readonly string[];
  cwd?: string;
  journal?: Journal;
}): Match[] {
  const disabled = new Set(opts.disable ?? []);
  const matches: Match[] = [];
  const byId = notesById(opts.notes);
  for (const lane of opts.lanes) {
    if (disabled.has(lane.lane)) continue;
    for (const note of opts.notes) {
      if (!matchesPredicates(note.frontMatter, lane.trigger)) continue;
      if (
        lane.need !== undefined &&
        !matchesNeed({
          need: lane.need,
          frontMatter: note.frontMatter,
          cwd: opts.cwd,
          byId,
        })
      ) {
        opts.journal?.record({
          kind: "skip",
          lane: lane.lane,
          path: note.path,
          reason: "need",
        });
        continue;
      }
      matches.push({ lane, note });
    }
  }
  return matches;
}

function matchesNeed(opts: {
  need: Record<string, YamlValue>;
  frontMatter: Record<string, YamlValue>;
  cwd: string | undefined;
  byId: ReadonlyMap<string, ScannedNote>;
}): boolean {
  const scalars: Record<string, YamlValue> = {};
  for (const [key, expected] of Object.entries(opts.need)) {
    if (key === "exists") {
      if (!pathsPresent(opts.cwd, expected, true)) return false;
      continue;
    }
    if (key === "absent") {
      if (!pathsPresent(opts.cwd, expected, false)) return false;
      continue;
    }
    if (key === "status-of") {
      if (!statusOfMatches(expected, opts.frontMatter, opts.byId)) return false;
      continue;
    }
    scalars[key] = expected;
  }
  return matchesPredicates(opts.frontMatter, scalars);
}

function notesById(
  notes: readonly ScannedNote[],
): ReadonlyMap<string, ScannedNote> {
  const byId = new Map<string, ScannedNote>();
  for (const note of notes) {
    const id = note.frontMatter.id;
    if (typeof id !== "string" || id === "" || byId.has(id)) continue;
    byId.set(id, note);
  }
  return byId;
}

function statusOfMatches(
  value: YamlValue,
  frontMatter: Record<string, YamlValue>,
  byId: ReadonlyMap<string, ScannedNote>,
): boolean {
  const expectedByField = statusOfMap(value);
  if (expectedByField === undefined) return false;
  for (const [field, expected] of Object.entries(expectedByField)) {
    for (const id of idList(frontMatter[field])) {
      const target = byId.get(id);
      if (target === undefined) return false;
      if (!Object.is(target.frontMatter.status, expected)) return false;
    }
  }
  return true;
}

function statusOfMap(value: YamlValue): Record<string, string> | undefined {
  if (value === null || Array.isArray(value) || typeof value !== "object") {
    return undefined;
  }
  const out: Record<string, string> = {};
  for (const [field, expected] of Object.entries(value)) {
    if (typeof expected !== "string") return undefined;
    out[field] = expected;
  }
  return out;
}

function idList(value: YamlValue | undefined): string[] {
  if (value === undefined || value === null) return [];
  if (typeof value === "string") {
    return value === "" || value === "none" ? [] : [value];
  }
  if (!Array.isArray(value)) return [];
  const ids: string[] = [];
  for (const item of value) {
    if (typeof item !== "string" || item === "" || item === "none") continue;
    ids.push(item);
  }
  return ids;
}

function pathsPresent(
  cwd: string | undefined,
  value: YamlValue,
  want: boolean,
): boolean {
  const paths = pathList(value);
  if (paths === undefined) return false;
  if (cwd === undefined) return false;
  for (const rel of paths) {
    if (existsSync(join(cwd, rel)) !== want) return false;
  }
  return true;
}

function pathList(value: YamlValue): string[] | undefined {
  if (typeof value === "string") return [value];
  if (!Array.isArray(value)) return undefined;
  const paths: string[] = [];
  for (const item of value) {
    if (typeof item !== "string") return undefined;
    paths.push(item);
  }
  return paths;
}

function matchesPredicates(
  frontMatter: Record<string, YamlValue>,
  predicates: Record<string, YamlValue>,
): boolean {
  for (const [key, expected] of Object.entries(predicates)) {
    if (!yamlEqual(frontMatter[key], expected)) return false;
  }
  return true;
}

function yamlEqual(left: YamlValue | undefined, right: YamlValue): boolean {
  if (Object.is(left, right)) return true;
  if (left === undefined || left === null || right === null) return false;
  if (Array.isArray(left) || Array.isArray(right)) return false;
  if (typeof left === "object" || typeof right === "object") return false;
  return false;
}
