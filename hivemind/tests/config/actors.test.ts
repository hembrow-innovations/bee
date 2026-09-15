import assert from "node:assert/strict";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { withTemp } from "../with-temp.ts";
import { listActorDocuments } from "../../src/config/actors.ts";

test("listActorDocuments returns [] when the directory is missing", () => {
  const dir = join(withTemp("hivemind-actors-missing-"), "actors");
  assert.deepEqual(listActorDocuments(dir), []);
});

test("listActorDocuments throws when the path is not a directory", () => {
  const cwd = withTemp("hivemind-actors-file-");
  const file = join(cwd, "actors");
  writeFileSync(file, "not a directory\n");
  assert.throws(
    () => listActorDocuments(file),
    /\.hivemind\/actors must be a directory/,
  );
});

test("listActorDocuments lists yaml files by name with stem and parsed raw", () => {
  const dir = withTemp("hivemind-actors-list-");
  writeFileSync(join(dir, "z.yaml"), "cmd: /bin/echo z\n");
  writeFileSync(join(dir, "a.yml"), "cmd: /bin/echo a\n");
  writeFileSync(join(dir, "readme.md"), "ignore me\n");
  mkdirSync(join(dir, "nested.yaml"));
  writeFileSync(
    join(dir, "nested.yaml", "inner.yaml"),
    "cmd: /bin/echo inner\n",
  );

  assert.deepEqual(listActorDocuments(dir), [
    { file: "a.yml", stem: "a", raw: { cmd: "/bin/echo a" } },
    { file: "z.yaml", stem: "z", raw: { cmd: "/bin/echo z" } },
  ]);
});
