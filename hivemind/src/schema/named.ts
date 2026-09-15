import { QUARANTINE_KEYS } from "./quarantine.ts";

export function namedAllowlist(name: string): ReadonlySet<string> {
  if (name === "quarantine") return QUARANTINE_KEYS;
  throw new Error(`Unknown folder schema "${name}"`);
}
