import type { SpawnSpec } from "../config/loadConfig.ts";

export type InterpolateResult =
  | { kind: "ok"; value: string }
  | { kind: "skip" };

export function interpolate(opts: {
  template: string;
  cwd: string;
  lane: string;
  spec: SpawnSpec;
  env: NodeJS.ProcessEnv;
  runId?: string;
  stage?: string;
  path?: string;
  redactEnv?: boolean;
}): InterpolateResult {
  let skip = false;
  const value = opts.template.replace(
    /\{\{([^}]*)\}\}/g,
    (_full, name: string) => {
      if (skip) return "";
      const resolved = resolvePlaceholder({
        name,
        cwd: opts.cwd,
        lane: opts.lane,
        spec: opts.spec,
        env: opts.env,
        runId: opts.runId,
        stage: opts.stage,
        path: opts.path,
        redactEnv: opts.redactEnv,
      });
      if (resolved.kind === "skip") {
        skip = true;
        return "";
      }
      return resolved.value;
    },
  );
  if (skip) return { kind: "skip" };
  if (value.includes("{{")) return { kind: "skip" };
  return { kind: "ok", value };
}

function resolvePlaceholder(opts: {
  name: string;
  cwd: string;
  lane: string;
  spec: SpawnSpec;
  env: NodeJS.ProcessEnv;
  runId?: string;
  stage?: string;
  path?: string;
  redactEnv?: boolean;
}): InterpolateResult {
  if (opts.name.startsWith("env.")) {
    const key = opts.name.slice("env.".length);
    if (key === "") return { kind: "skip" };
    const value = opts.env[key];
    if (value === undefined || value === "") return { kind: "skip" };
    if (opts.redactEnv === true) return { kind: "ok", value: "***" };
    return { kind: "ok", value };
  }
  if (opts.name === "cwd") return { kind: "ok", value: opts.cwd };
  if (opts.name === "lane") return { kind: "ok", value: opts.lane };
  if (opts.name === "run-id") {
    if (opts.runId === undefined || opts.runId === "") return { kind: "skip" };
    return { kind: "ok", value: opts.runId };
  }
  if (opts.name === "stage") {
    if (opts.stage === undefined || opts.stage === "") return { kind: "skip" };
    return { kind: "ok", value: opts.stage };
  }
  if (opts.name === "path") {
    if (opts.path === undefined || opts.path === "") return { kind: "skip" };
    return { kind: "ok", value: opts.path };
  }
  if (opts.name === "agent") {
    if (opts.spec.agent === undefined || opts.spec.agent === "") {
      return { kind: "skip" };
    }
    return { kind: "ok", value: opts.spec.agent };
  }
  if (opts.name === "prompt") {
    if (opts.spec.prompt === undefined || opts.spec.prompt === "") {
      return { kind: "skip" };
    }
    return { kind: "ok", value: opts.spec.prompt };
  }
  if (opts.name === "exclusive") {
    return { kind: "ok", value: opts.spec.exclusive.join(" ") };
  }
  const named = opts.spec.scalars[opts.name];
  if (named === undefined) return { kind: "skip" };
  return { kind: "ok", value: named };
}
