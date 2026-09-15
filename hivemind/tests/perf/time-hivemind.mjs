#!/usr/bin/env node
import { readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");

function listTestFiles(dir) {
  const out = [];
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, ent.name);
    if (ent.isDirectory()) {
      out.push(...listTestFiles(full));
      continue;
    }
    if (ent.isFile() && ent.name.endsWith(".test.ts")) out.push(full);
  }
  return out.sort();
}

const tests = listTestFiles(join(ROOT, "tests"));
if (tests.length === 0) {
  console.error("no *.test.ts files under tests/");
  process.exit(1);
}

const started = Date.now();
const result = spawnSync(
  process.execPath,
  ["--experimental-strip-types", "--test", ...tests],
  { cwd: ROOT, stdio: "inherit" },
);
const ms = Date.now() - started;
console.log(`hivemind-tests ${ms}ms status ${result.status ?? 1}`);
process.exit(result.status ?? 1);
