import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { parseYaml, type YamlValue } from "../yaml/yaml.ts";

export type ActorDocument = {
  file: string;
  stem: string;
  raw: Record<string, YamlValue>;
};

export function listActorDocuments(dir: string): ActorDocument[] {
  if (!existsSync(dir)) return [];
  if (!statSync(dir).isDirectory()) {
    throw new Error(".hivemind/actors must be a directory");
  }
  const names = readdirSync(dir)
    .filter((name) => name.endsWith(".yaml") || name.endsWith(".yml"))
    .sort();
  const out: ActorDocument[] = [];
  for (const name of names) {
    const file = join(dir, name);
    if (!statSync(file).isFile()) continue;
    const raw = parseYaml(readFileSync(file, "utf8"));
    out.push({
      file: name,
      stem: name.replace(/\.ya?ml$/, ""),
      raw,
    });
  }
  return out;
}
