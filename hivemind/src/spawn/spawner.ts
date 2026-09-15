import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import type { SpawnSpec } from "../config/loadConfig.ts";
import type { Journal } from "../journal/journal.ts";
import { interpolate } from "./interpolator.ts";
import { tokenize } from "./tokenizer.ts";

export type SpawnHandle = {
  wait: Promise<number>;
  kill: () => void;
  pid?: number;
};

export type SpawnChild = (opts: {
  argv: readonly string[];
  cwd: string;
  env?: NodeJS.ProcessEnv;
  onEvent?: () => void;
}) => SpawnHandle;

export type RenderArgvResult =
  | { kind: "ok"; argvList: string[][] }
  | { kind: "skip"; reason: "missing-prompt" | "cmd-skip" };

export function renderArgv(opts: {
  specs: readonly SpawnSpec[];
  cwd: string;
  lane: string;
  env: NodeJS.ProcessEnv;
  runId?: string;
  path?: string;
  stages?: readonly (string | undefined)[];
  redactEnv?: boolean;
}): RenderArgvResult {
  const argvList: string[][] = [];
  let index = 0;
  for (const spec of opts.specs) {
    const argv = cmdArgv({
      spec,
      lane: opts.lane,
      cwd: opts.cwd,
      env: opts.env,
      runId: opts.runId,
      path: opts.path,
      stage: opts.stages?.[index],
      redactEnv: opts.redactEnv,
    });
    if (argv.kind === "skip") return argv;
    argvList.push(argv.argv);
    index += 1;
  }
  return { kind: "ok", argvList };
}

const KILL_GRACE_MS = 2000;

export function startSpawn(opts: {
  argvList: readonly (readonly string[])[];
  cwd: string;
  env: NodeJS.ProcessEnv;
  spawnChild?: SpawnChild;
  journal?: Journal;
  lane: string;
  path: string;
  runId: string;
  stages?: readonly (string | undefined)[];
  timeoutMs?: number;
  stallMs?: number;
}): SpawnHandle {
  const spawnChild = opts.spawnChild ?? spawnArgv;
  let currentKill = noop;
  let cancelled = false;
  let exitReason: "timeout" | "stall" | undefined;
  let timeoutTimer: ReturnType<typeof setTimeout> | undefined;
  let stallTimer: ReturnType<typeof setTimeout> | undefined;
  const childEnv = {
    ...opts.env,
    HIVEMIND_RUN_ID: opts.runId,
    HIVEMIND_LANE: opts.lane,
    HIVEMIND_PATH: opts.path,
  };
  const clearTimers = () => {
    if (timeoutTimer !== undefined) clearTimeout(timeoutTimer);
    if (stallTimer !== undefined) clearTimeout(stallTimer);
    timeoutTimer = undefined;
    stallTimer = undefined;
  };
  const fire = (reason: "timeout" | "stall") => {
    if (cancelled) return;
    cancelled = true;
    exitReason = reason;
    clearTimers();
    currentKill();
  };
  const armStall = () => {
    if (stallTimer !== undefined) clearTimeout(stallTimer);
    stallTimer = undefined;
    const ms = opts.stallMs ?? 0;
    if (ms <= 0 || cancelled) return;
    stallTimer = setTimeout(() => fire("stall"), ms);
  };
  const onEvent = () => {
    if (cancelled) return;
    armStall();
  };
  const timeoutMs = opts.timeoutMs ?? 0;
  if (timeoutMs > 0) {
    timeoutTimer = setTimeout(() => fire("timeout"), timeoutMs);
  }
  armStall();
  const firstArgv = opts.argvList[0];
  if (firstArgv === undefined) {
    clearTimers();
    return { wait: Promise.resolve(0), kill: noop };
  }
  const first = spawnChild({
    argv: firstArgv,
    cwd: opts.cwd,
    env: childEnv,
    onEvent,
  });
  currentKill = first.kill;
  const firstIdentity = spawnExitIdentity({
    stage: opts.stages?.[0],
    pid: first.pid,
  });
  opts.journal?.record({
    kind: "spawn",
    lane: opts.lane,
    path: opts.path,
    runId: opts.runId,
    ...firstIdentity,
  });
  onEvent();
  const wait = (async () => {
    let status: number;
    try {
      status = await first.wait;
    } catch {
      status = 1;
    }
    opts.journal?.record({
      kind: "exit",
      lane: opts.lane,
      path: opts.path,
      runId: opts.runId,
      status,
      ...firstIdentity,
      ...(exitReason !== undefined ? { reason: exitReason } : {}),
    });
    if (cancelled || status !== 0) {
      clearTimers();
      return status;
    }
    let index = 1;
    for (const argv of opts.argvList.slice(1)) {
      if (cancelled) {
        clearTimers();
        return 1;
      }
      const handle = spawnChild({
        argv,
        cwd: opts.cwd,
        env: childEnv,
        onEvent,
      });
      currentKill = handle.kill;
      const identity = spawnExitIdentity({
        stage: opts.stages?.[index],
        pid: handle.pid,
      });
      opts.journal?.record({
        kind: "spawn",
        lane: opts.lane,
        path: opts.path,
        runId: opts.runId,
        ...identity,
      });
      onEvent();
      try {
        status = await handle.wait;
      } catch {
        status = 1;
      }
      opts.journal?.record({
        kind: "exit",
        lane: opts.lane,
        path: opts.path,
        runId: opts.runId,
        status,
        ...identity,
        ...(exitReason !== undefined ? { reason: exitReason } : {}),
      });
      index += 1;
      if (cancelled || status !== 0) {
        clearTimers();
        return status;
      }
    }
    clearTimers();
    return 0;
  })();
  return {
    wait,
    kill: () => {
      cancelled = true;
      clearTimers();
      currentKill();
    },
    pid: first.pid,
  };
}

function spawnArgv(opts: {
  argv: readonly string[];
  cwd: string;
  env?: NodeJS.ProcessEnv;
  onEvent?: () => void;
}): SpawnHandle {
  const command = opts.argv[0];
  if (command === undefined) {
    return { wait: Promise.resolve(0), kill: noop };
  }
  const args = opts.argv.slice(1);
  const child = spawn(command, args, {
    cwd: opts.cwd,
    env: opts.env,
    shell: false,
    stdio: "inherit",
  });
  let killTimer: ReturnType<typeof setTimeout> | undefined;
  const wait = new Promise<number>((resolve, reject) => {
    child.on("error", reject);
    child.on("exit", (code: number | null) => {
      if (killTimer !== undefined) clearTimeout(killTimer);
      resolve(code ?? 1);
    });
  });
  return {
    wait,
    kill: () => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      child.kill("SIGTERM");
      killTimer = setTimeout(() => {
        if (child.exitCode === null && child.signalCode === null) {
          child.kill("SIGKILL");
        }
      }, KILL_GRACE_MS);
    },
    pid: child.pid,
  };
}

function spawnExitIdentity(opts: {
  stage: string | undefined;
  pid: number | undefined;
}): { stage?: string; pid?: number } {
  const identity: { stage?: string; pid?: number } = {};
  if (opts.stage !== undefined && opts.stage !== "") {
    identity.stage = opts.stage;
  }
  if (opts.pid !== undefined) identity.pid = opts.pid;
  return identity;
}

function cmdArgv(opts: {
  spec: SpawnSpec;
  lane: string;
  cwd: string;
  env: NodeJS.ProcessEnv;
  runId?: string;
  path?: string;
  stage?: string;
  redactEnv?: boolean;
}):
  | { kind: "ok"; argv: string[] }
  | { kind: "skip"; reason: "missing-prompt" | "cmd-skip" } {
  if (opts.spec.prompt !== undefined && opts.spec.prompt !== "") {
    if (!existsSync(join(opts.cwd, opts.spec.prompt))) {
      return { kind: "skip", reason: "missing-prompt" };
    }
  }
  if (typeof opts.spec.cmd !== "string") {
    const argv: string[] = [];
    for (const part of opts.spec.cmd) {
      const rendered = interpolate({
        template: part,
        cwd: opts.cwd,
        lane: opts.lane,
        spec: opts.spec,
        env: opts.env,
        runId: opts.runId,
        path: opts.path,
        stage: opts.stage,
        redactEnv: opts.redactEnv,
      });
      if (rendered.kind === "skip") {
        return { kind: "skip", reason: "cmd-skip" };
      }
      argv.push(rendered.value);
    }
    return { kind: "ok", argv };
  }
  const rendered = interpolate({
    template: opts.spec.cmd,
    cwd: opts.cwd,
    lane: opts.lane,
    spec: opts.spec,
    env: opts.env,
    runId: opts.runId,
    path: opts.path,
    stage: opts.stage,
    redactEnv: opts.redactEnv,
  });
  if (rendered.kind === "skip") return { kind: "skip", reason: "cmd-skip" };
  const tokens = tokenize(rendered.value);
  if (tokens.kind === "fail") return { kind: "skip", reason: "cmd-skip" };
  return { kind: "ok", argv: tokens.argv };
}

function noop(): void {}
