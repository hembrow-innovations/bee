#!/usr/bin/env node
import { readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

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

const result = spawnSync(
  process.execPath,
  ["--experimental-strip-types", "--test", "--test-timeout=60000", ...tests],
  { cwd: ROOT, stdio: "inherit" },
);
if (result.status) process.exit(result.status);

process.exit(0);
