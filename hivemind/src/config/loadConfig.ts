import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parseYaml, type YamlValue } from "../yaml/yaml.ts";
import { listActorDocuments } from "./actors.ts";

export const CONFIG_REL = ".hivemind/hivemind.yaml";
export const ACTORS_REL = ".hivemind/actors";
export const STOP_REL = ".hivemind/STOP";

const CONFIG_KEYS = new Set([
  "watch",
  "folders",
  "lanes",
  "disable",
  "history",
  "actors",
  "stop",
]);

const ACTOR_KEYS = new Set([
  "cmd",
  "agent",
  "prompt",
  "scope",
  "exclusive",
  "claim-status",
]);

const LANE_KEYS = new Set([
  "type",
  "concurrency",
  "actor",
  "cmd",
  "agent",
  "prompt",
  "scope",
  "exclusive",
  "claim-status",
  "trigger",
  "need",
  "backoff",
  "cooldown",
  "ttl",
  "timeout",
  "stall",
  "audit",
  "stages",
]);

const STAGE_KEYS = new Set([
  "stage",
  "actor",
  "cmd",
  "agent",
  "prompt",
  "scope",
  "exclusive",
  "claim-status",
]);

export type SpawnSpec = {
  cmd: string | readonly string[];
  agent: string | undefined;
  prompt: string | undefined;
  exclusive: readonly string[];
  claimStatus: string;
  scalars: Record<string, string>;
};

export type LaneStage = SpawnSpec & { stage: string };

type LaneCommon = {
  lane: string;
  trigger: Record<string, YamlValue>;
  need: Record<string, YamlValue> | undefined;
  exclusive: readonly string[];
  claimStatus: string;
  backoffMs: number;
  cooldownMs: number;
  ttlMs: number;
  timeoutMs: number;
  stallMs: number;
  audit?: "git-diff";
  concurrency: number;
};

export type UnitLane = LaneCommon &
  SpawnSpec & {
    type: "single";
  };

export type PipelineLane = LaneCommon & {
  type: "pipeline";
  stages: readonly LaneStage[];
};

export type Lane = UnitLane | PipelineLane;

export type Actor = {
  cmd: string | readonly string[] | undefined;
  agent: string | undefined;
  prompt: string | undefined;
  exclusive: readonly string[];
  claimStatus: string | undefined;
  scalars: Record<string, string>;
};

export type HivemindConfig = {
  folders: YamlValue[];
  lanes: Lane[];
  disable: readonly string[];
  watch: readonly string[] | undefined;
  history: string | undefined;
  stop: string;
};

export function loadConfig(cwd: string): HivemindConfig {
  return loadConfigFile({
    file: join(cwd, CONFIG_REL),
    actorsDir: join(cwd, ACTORS_REL),
  });
}

export function loadConfigFile(opts: {
  file: string;
  actorsDir?: string;
}): HivemindConfig {
  if (!existsSync(opts.file)) {
    throw new Error(`Missing ${CONFIG_REL}`);
  }
  const raw = parseYaml(readFileSync(opts.file, "utf8"));
  for (const key of Object.keys(raw)) {
    if (!CONFIG_KEYS.has(key)) {
      throw new Error(`Unknown key "${key}"`);
    }
  }
  const actors = loadActors({
    yaml: raw.actors,
    dir: opts.actorsDir,
  });
  const lanes = parseLanes(raw.lanes, actors);
  const history = parseHistory(raw.history);
  if (history === undefined && lanes.some((lane) => lane.ttlMs > 0)) {
    throw new Error("ttl requires history");
  }
  return {
    folders: asList(raw.folders, "folders"),
    lanes,
    disable: parseStringList(raw.disable, "disable"),
    watch: parseWatch(raw.watch),
    history,
    stop: parseStop(raw.stop),
  };
}

function loadActors(opts: {
  yaml: YamlValue | undefined;
  dir: string | undefined;
}): Map<string, Actor> {
  const actors = new Map<string, Actor>();
  if (opts.dir !== undefined) {
    for (const doc of listActorDocuments(opts.dir)) {
      const parsed = parseActorDocument(doc.raw, doc.stem, doc.file);
      for (const [name, actor] of parsed) {
        if (actors.has(name)) {
          throw new Error(`Duplicate actor "${name}"`);
        }
        actors.set(name, actor);
      }
    }
  }
  if (opts.yaml !== undefined) {
    if (!isMap(opts.yaml)) {
      throw new Error('"actors" must be a map');
    }
    for (const [name, value] of Object.entries(opts.yaml)) {
      actors.set(name, parseActor(name, value));
    }
  }
  return actors;
}

function parseActorDocument(
  raw: Record<string, YamlValue>,
  stem: string,
  file: string,
): Map<string, Actor> {
  if (isCmdValue(raw.cmd)) {
    return new Map([[stem, parseActor(stem, raw)]]);
  }
  const out = new Map<string, Actor>();
  for (const [name, value] of Object.entries(raw)) {
    if (out.has(name)) {
      throw new Error(`Duplicate actor "${name}" in ${file}`);
    }
    out.set(name, parseActor(name, value));
  }
  return out;
}

function parseActor(name: string, value: YamlValue): Actor {
  if (!isMap(value)) {
    throw new Error(`actor "${name}" must be a map`);
  }
  rejectUnknownKeys(value, ACTOR_KEYS, `actor "${name}"`);
  const scope = parseStringList(value.scope, "scope");
  const exclusive = parseStringList(value.exclusive, "exclusive");
  if (value.scope !== undefined && value.exclusive !== undefined) {
    if (!sameStrings(scope, exclusive)) {
      throw new Error(`actor "${name}" exclusive and scope must be equal`);
    }
  }
  return {
    cmd: value.cmd === undefined ? undefined : parseCmd(value.cmd),
    agent: optionalString(value.agent),
    prompt: optionalString(value.prompt),
    exclusive: exclusive.length > 0 ? exclusive : scope,
    claimStatus: optionalString(value["claim-status"]),
    scalars: stringScalars(value),
  };
}

function parseLanes(
  value: YamlValue | undefined,
  actors: Map<string, Actor>,
): Lane[] {
  if (value === undefined) {
    throw new Error('"lanes" is required');
  }
  if (Array.isArray(value)) {
    throw new Error('"lanes" must be a map');
  }
  if (!isMap(value)) {
    throw new Error('"lanes" must be a map');
  }
  const lanes: Lane[] = [];
  for (const [id, item] of Object.entries(value)) {
    if (id === "") {
      throw new Error("lane id is required");
    }
    lanes.push(parseLane(id, item, actors));
  }
  return lanes;
}

function parseLane(
  id: string,
  item: YamlValue,
  actors: Map<string, Actor>,
): Lane {
  if (!isMap(item)) {
    throw new Error(`lane "${id}" must be a map`);
  }
  const type = item.type;
  if (type === "single") {
    return parseSingleLane(id, item, actors);
  }
  if (type === "pipeline") {
    return parsePipelineLane(id, item, actors);
  }
  if (type === undefined || type === "") {
    throw new Error(`lane "${id}" is missing type`);
  }
  throw new Error(`lane "${id}" has unknown type "${String(type)}"`);
}

function parseSingleLane(
  id: string,
  item: Record<string, YamlValue>,
  actors: Map<string, Actor>,
): UnitLane {
  rejectUnknownKeys(item, LANE_KEYS, `lane "${id}"`);
  if (item.stages !== undefined) {
    throw new Error(`lane "${id}" type single cannot have stages`);
  }
  const spawn = resolveSpawn(id, item, actors, { requireClaim: true });
  const trigger = parseMap(item.trigger, "trigger");
  rejectClaimEqualsTrigger(id, spawn.claimStatus, trigger);
  return {
    type: "single",
    lane: id,
    cmd: spawn.cmd,
    agent: spawn.agent,
    prompt: spawn.prompt,
    exclusive: spawn.exclusive,
    claimStatus: spawn.claimStatus,
    scalars: spawn.scalars,
    trigger,
    need: item.need === undefined ? undefined : parseMap(item.need, "need"),
    backoffMs: parseDuration(item.backoff, id, "backoff"),
    cooldownMs: parseDuration(item.cooldown, id, "cooldown"),
    ttlMs: parseDuration(item.ttl, id, "ttl"),
    timeoutMs: parseDuration(item.timeout, id, "timeout"),
    stallMs: parseDuration(item.stall, id, "stall"),
    audit: parseAudit(item.audit, id),
    concurrency: parseConcurrency(item.concurrency, id),
  };
}

function parsePipelineLane(
  id: string,
  item: Record<string, YamlValue>,
  actors: Map<string, Actor>,
): PipelineLane {
  rejectUnknownKeys(item, LANE_KEYS, `lane "${id}"`);
  const trigger = parseMap(item.trigger, "trigger");
  const defaults = resolveSpawnDefaults(id, item, actors);
  const claimStatus = requiredClaim(id, item, defaults.actor);
  rejectClaimEqualsTrigger(id, claimStatus, trigger);
  const exclusive = resolveExclusive(item, defaults.actor);
  const stages = parseStages(id, item.stages, actors, defaults, claimStatus);
  if (stages.length === 0) {
    throw new Error(`lane "${id}" pipeline has no stages`);
  }
  return {
    type: "pipeline",
    lane: id,
    trigger,
    need: item.need === undefined ? undefined : parseMap(item.need, "need"),
    exclusive,
    claimStatus,
    backoffMs: parseDuration(item.backoff, id, "backoff"),
    cooldownMs: parseDuration(item.cooldown, id, "cooldown"),
    ttlMs: parseDuration(item.ttl, id, "ttl"),
    timeoutMs: parseDuration(item.timeout, id, "timeout"),
    stallMs: parseDuration(item.stall, id, "stall"),
    audit: parseAudit(item.audit, id),
    concurrency: parseConcurrency(item.concurrency, id),
    stages,
  };
}

function parseStages(
  laneId: string,
  value: YamlValue | undefined,
  actors: Map<string, Actor>,
  defaults: SpawnDefaults,
  claimStatus: string,
): LaneStage[] {
  if (value === undefined) {
    throw new Error(`lane "${laneId}" pipeline is missing stages`);
  }
  if (!Array.isArray(value)) {
    throw new Error(`lane "${laneId}" stages must be a list`);
  }
  const stages: LaneStage[] = [];
  const ids = new Set<string>();
  for (const item of value) {
    if (!isMap(item)) {
      throw new Error(`lane "${laneId}" stages entries must be maps`);
    }
    if (typeof item.stage !== "string" || item.stage === "") {
      throw new Error(`lane "${laneId}" stage id is required`);
    }
    const stageId = item.stage;
    if (ids.has(stageId)) {
      throw new Error(`lane "${laneId}" duplicate stage "${stageId}"`);
    }
    ids.add(stageId);
    rejectUnknownKeys(item, STAGE_KEYS, `stage "${stageId}"`);
    const spawn = resolveSpawn(stageId, item, actors, {
      requireClaim: false,
      defaults,
      claimStatus: optionalString(item["claim-status"]) ?? claimStatus,
    });
    stages.push({
      stage: stageId,
      cmd: spawn.cmd,
      agent: spawn.agent,
      prompt: spawn.prompt,
      exclusive: spawn.exclusive,
      claimStatus: spawn.claimStatus,
      scalars: { ...spawn.scalars, stage: stageId },
    });
  }
  return stages;
}

type SpawnDefaults = {
  actor: Actor | undefined;
  cmd: string | readonly string[] | undefined;
  agent: string | undefined;
  prompt: string | undefined;
  exclusive: readonly string[];
  scalars: Record<string, string>;
};

function resolveSpawnDefaults(
  id: string,
  item: Record<string, YamlValue>,
  actors: Map<string, Actor>,
): SpawnDefaults {
  const actor = lookupActor(id, item.actor, actors);
  return {
    actor,
    cmd: item.cmd === undefined ? actor?.cmd : parseCmd(item.cmd),
    agent: item.agent === undefined ? actor?.agent : optionalString(item.agent),
    prompt:
      item.prompt === undefined ? actor?.prompt : optionalString(item.prompt),
    exclusive: resolveExclusive(item, actor),
    scalars: { ...(actor?.scalars ?? {}), ...stringScalars(item) },
  };
}

function resolveSpawn(
  id: string,
  item: Record<string, YamlValue>,
  actors: Map<string, Actor>,
  opts: {
    requireClaim: boolean;
    defaults?: SpawnDefaults;
    claimStatus?: string;
  },
): SpawnSpec {
  const actor = lookupActor(id, item.actor, actors) ?? opts.defaults?.actor;
  const cmd =
    item.cmd === undefined
      ? (actor?.cmd ?? opts.defaults?.cmd)
      : parseCmd(item.cmd);
  if (cmd === undefined) {
    throw new Error(`"${id}" is missing cmd`);
  }
  const inheritAgentPrompt = item.cmd === undefined;
  const agent =
    item.agent === undefined
      ? inheritAgentPrompt
        ? (actor?.agent ?? opts.defaults?.agent)
        : undefined
      : optionalString(item.agent);
  const prompt =
    item.prompt === undefined
      ? inheritAgentPrompt
        ? (actor?.prompt ?? opts.defaults?.prompt)
        : undefined
      : optionalString(item.prompt);
  const exclusive = resolveExclusive(item, actor, opts.defaults);
  const claimStatus =
    opts.claimStatus ??
    (opts.requireClaim
      ? requiredClaim(id, item, actor)
      : (optionalString(item["claim-status"]) ??
        actor?.claimStatus ??
        opts.defaults?.actor?.claimStatus ??
        ""));
  const scalars = {
    ...(opts.defaults?.scalars ?? {}),
    ...(actor?.scalars ?? {}),
    ...stringScalars(item),
  };
  return {
    cmd,
    agent,
    prompt,
    exclusive,
    claimStatus,
    scalars,
  };
}

function lookupActor(
  id: string,
  value: YamlValue | undefined,
  actors: Map<string, Actor>,
): Actor | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "string" || value === "") {
    throw new Error(`"${id}" actor must be a string`);
  }
  const actor = actors.get(value);
  if (actor === undefined) {
    throw new Error(`unknown actor "${value}"`);
  }
  return actor;
}

function requiredClaim(
  id: string,
  item: Record<string, YamlValue>,
  actor: Actor | undefined,
): string {
  const fromItem = optionalString(item["claim-status"]);
  const claim = fromItem ?? actor?.claimStatus;
  if (claim === undefined || claim === "") {
    throw new Error(`lane "${id}" is missing claim-status`);
  }
  return claim;
}

function rejectClaimEqualsTrigger(
  id: string,
  claimStatus: string,
  trigger: Record<string, YamlValue>,
): void {
  if (typeof trigger.status === "string" && claimStatus === trigger.status) {
    throw new Error(`lane "${id}" claim-status equals trigger.status`);
  }
}

function resolveExclusive(
  item: Record<string, YamlValue>,
  actor: Actor | undefined,
  defaults?: SpawnDefaults,
): readonly string[] {
  const scope = parseStringList(item.scope, "scope");
  const exclusive = parseStringList(item.exclusive, "exclusive");
  if (item.scope !== undefined && item.exclusive !== undefined) {
    if (!sameStrings(scope, exclusive)) {
      throw new Error("exclusive and scope must be equal");
    }
  }
  if (exclusive.length > 0) return exclusive;
  if (scope.length > 0) return scope;
  if (actor !== undefined && actor.exclusive.length > 0) return actor.exclusive;
  return defaults?.exclusive ?? [];
}

function rejectUnknownKeys(
  item: Record<string, YamlValue>,
  allowed: ReadonlySet<string>,
  label: string,
): void {
  for (const [key, value] of Object.entries(item)) {
    if (allowed.has(key)) continue;
    if (typeof value === "string" && key !== "audit") continue;
    throw new Error(`${label} unknown key "${key}"`);
  }
}

function stringScalars(
  item: Record<string, YamlValue>,
): Record<string, string> {
  const scalars: Record<string, string> = {};
  for (const [key, value] of Object.entries(item)) {
    if (typeof value === "string" && key !== "audit") scalars[key] = value;
  }
  return scalars;
}

function parseHistory(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "string" || value === "") {
    throw new Error("history must be a path");
  }
  return value;
}

function parseStop(value: unknown): string {
  if (value === undefined) return STOP_REL;
  if (typeof value !== "string" || value === "") {
    throw new Error("stop must be a path");
  }
  return value;
}

function parseWatch(value: unknown): string[] | undefined {
  if (value === undefined) return undefined;
  const list = parseStringList(value, "watch");
  if (list.length === 0) return undefined;
  return list.map(normalizeWatchRoot);
}

function normalizeWatchRoot(path: string): string {
  return path.replace(/\/\*\*(\/\*\.md)?$/, "").replace(/\/+$/, "");
}

function asList(value: unknown, key: string): YamlValue[] {
  if (value === undefined) {
    throw new Error(`"${key}" is required`);
  }
  if (!Array.isArray(value)) {
    throw new Error(`"${key}" must be a list`);
  }
  return value;
}

function parseConcurrency(value: unknown, laneId: string): number {
  if (value === undefined) return 1;
  if (typeof value !== "number" || !Number.isInteger(value) || value < 1) {
    throw new Error(`lane "${laneId}" concurrency must be a positive integer`);
  }
  return value;
}

function parseStringList(value: unknown, key: string): string[] {
  if (value === undefined) return [];
  if (!Array.isArray(value)) {
    throw new Error(`"${key}" must be a list of strings`);
  }
  const out: string[] = [];
  for (const item of value) {
    if (typeof item !== "string") {
      throw new Error(`"${key}" must be a list of strings`);
    }
    out.push(item);
  }
  return out;
}

function parseAudit(value: unknown, laneId: string): "git-diff" | undefined {
  if (value === undefined) return undefined;
  if (value === "git-diff") return "git-diff";
  throw new Error(`lane "${laneId}" audit is invalid`);
}

function parseDuration(value: unknown, laneId: string, field: string): number {
  if (value === undefined || value === 0 || value === "0") return 0;
  if (typeof value === "string") {
    const match = value.match(/^(\d+)([smh])$/);
    if (match !== null && match[1] !== undefined && match[2] !== undefined) {
      const n = Number(match[1]);
      if (match[2] === "s") return n * 1000;
      if (match[2] === "m") return n * 60_000;
      return n * 3_600_000;
    }
  }
  throw new Error(`lane "${laneId}" ${field} is invalid`);
}

function parseCmd(value: unknown): string | readonly string[] {
  if (typeof value === "string" && value !== "") return value;
  if (Array.isArray(value) && value.length > 0) {
    const parts: string[] = [];
    for (const item of value) {
      if (typeof item !== "string") {
        throw new Error("cmd list items must be strings");
      }
      parts.push(item);
    }
    return parts;
  }
  throw new Error("cmd is required");
}

function isCmdValue(value: unknown): boolean {
  if (typeof value === "string" && value !== "") return true;
  return Array.isArray(value) && value.length > 0;
}

function parseMap(
  value: YamlValue | undefined,
  key: string,
): Record<string, YamlValue> {
  if (!isMap(value)) {
    throw new Error(`"${key}" must be a map`);
  }
  return value;
}

function optionalString(value: unknown): string | undefined {
  if (value === undefined) return undefined;
  if (typeof value !== "string") {
    throw new Error("expected a string");
  }
  return value;
}

function sameStrings(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((item, i) => item === b[i]);
}

function isMap(value: unknown): value is Record<string, YamlValue> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
