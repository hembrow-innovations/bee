export type YamlScalar = string | number | boolean | null;
type YamlMap = { [key: string]: YamlValue };
export type YamlValue = YamlScalar | YamlValue[] | YamlMap;

type YamlTok = {
  indent: number;
  raw: string;
  isList: boolean;
  key: string | null;
  inline: string;
};

const JSON_NUMBER = /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?$/;

export function parseYaml(text: string): Record<string, YamlValue> {
  const toks = tokenizeYaml(text);
  if (toks.length === 0) return {};
  if (toks[0].indent !== 0) {
    throw new Error(`Unexpected indent: ${toks[0].raw}`);
  }
  const parsed = parseMap(toks, 0, 0);
  if (parsed.next !== toks.length) {
    throw new Error(`Cannot parse YAML: ${toks[parsed.next].raw}`);
  }
  return parsed.value;
}

function tokenizeYaml(text: string): YamlTok[] {
  const toks: YamlTok[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = stripComment(raw);
    if (!line.trim()) continue;
    rejectUnknown(line, raw);

    const indent = line.match(/^(\s*)/)?.[1].length ?? 0;
    const body = line.slice(indent);

    const list = body.match(/^-(\s+(.*))?$/);
    if (list) {
      const rest = (list[2] ?? "").trim();
      const kv =
        rest.match(/^([^:]+?):\s+(.*)$/) ?? rest.match(/^([^:]+?):\s*$/);
      if (kv) {
        toks.push({
          indent,
          raw,
          isList: true,
          key: kv[1].trim(),
          inline: kv[2],
        });
      } else {
        toks.push({
          indent,
          raw,
          isList: true,
          key: null,
          inline: rest,
        });
      }
      continue;
    }

    const kv = body.match(/^([^:]+?)\s*:\s*(.*)$/);
    if (!kv) throw new Error(`Cannot parse YAML: ${raw}`);
    toks.push({
      indent,
      raw,
      isList: false,
      key: kv[1].trim(),
      inline: kv[2],
    });
  }
  return toks;
}

function parseMap(
  toks: YamlTok[],
  start: number,
  indent: number,
): { value: YamlMap; next: number } {
  const value: YamlMap = {};
  let i = start;
  while (i < toks.length) {
    const t = toks[i];
    if (t.indent < indent) break;
    if (t.indent > indent) {
      throw new Error(`Unexpected indent: ${t.raw}`);
    }
    if (t.isList) throw new Error(`List item without a key: ${t.raw}`);
    if (t.key == null) throw new Error(`Cannot parse YAML: ${t.raw}`);

    if (t.inline !== "") {
      const peek = toks[i + 1];
      if (peek && peek.indent > t.indent) {
        throw new Error(`Mixed value and nested for "${t.key}": ${t.raw}`);
      }
      value[t.key] = parseScalar(t.inline, t.raw);
      i += 1;
      continue;
    }

    const child = parseChildren(toks, i, t.indent);
    i = child.next;
    if (!child.omit) value[t.key] = child.value;
  }
  return { value, next: i };
}

function parseList(
  toks: YamlTok[],
  start: number,
  indent: number,
): { value: YamlValue[]; next: number } {
  const value: YamlValue[] = [];
  let i = start;
  while (i < toks.length) {
    const t = toks[i];
    if (t.indent < indent) break;
    if (t.indent > indent) {
      throw new Error(`Unexpected indent: ${t.raw}`);
    }
    if (!t.isList) break;

    if (t.key != null) {
      const item: YamlMap = {};
      if (t.inline === "") {
        const child = parseChildren(toks, i, t.indent);
        i = child.next;
        if (!child.omit) item[t.key] = child.value;
      } else {
        item[t.key] = parseScalar(t.inline, t.raw);
        i += 1;
      }
      if (i < toks.length && toks[i].indent > indent && !toks[i].isList) {
        const rest = parseMap(toks, i, toks[i].indent);
        for (const [k, v] of Object.entries(rest.value)) item[k] = v;
        i = rest.next;
      }
      value.push(item);
      continue;
    }

    if (t.inline !== "") {
      const peek = toks[i + 1];
      if (peek && peek.indent > t.indent) {
        throw new Error(`Mixed value and nested: ${t.raw}`);
      }
      const item = parseScalar(t.inline, t.raw);
      if (item !== "" && item != null) value.push(item);
      i += 1;
      continue;
    }

    const child = parseChildren(toks, i, t.indent);
    i = child.next;
    if (!child.omit) value.push(child.value);
  }
  return { value, next: i };
}

function parseChildren(
  toks: YamlTok[],
  parentIndex: number,
  parentIndent: number,
): { value: YamlValue; next: number; omit: boolean } {
  const nextTok = toks[parentIndex + 1];
  if (!nextTok || nextTok.indent <= parentIndent) {
    return { value: null, next: parentIndex + 1, omit: true };
  }
  if (nextTok.isList) {
    const list = parseList(toks, parentIndex + 1, nextTok.indent);
    return { value: list.value, next: list.next, omit: false };
  }
  const map = parseMap(toks, parentIndex + 1, nextTok.indent);
  return { value: map.value, next: map.next, omit: false };
}

function stripComment(line: string): string {
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (c === "'" && !inDouble) inSingle = !inSingle;
    else if (c === '"' && !inSingle) inDouble = !inDouble;
    else if (c === "#" && !inSingle && !inDouble) return line.slice(0, i);
  }
  return line;
}

function rejectUnknown(line: string, raw: string): void {
  if (/(^|\s)[&*][A-Za-z_]/.test(line)) {
    throw new Error(`YAML anchors are not supported: ${raw}`);
  }
  if (/:\s*[|>][-+]?\s*$/.test(line)) {
    throw new Error(`Block scalars are not supported: ${raw}`);
  }
  const brace = line.indexOf("{");
  if (brace !== -1 && !inQuotes(line, brace)) {
    const body = line.trim();
    if (body !== "{}" && !/:\s*\{\}\s*$/.test(body)) {
      throw new Error(`Nested maps are not supported: ${raw}`);
    }
  }
}

function inQuotes(line: string, idx: number): boolean {
  let inSingle = false;
  let inDouble = false;
  for (let i = 0; i < idx; i++) {
    const c = line[i];
    if (c === "'" && !inDouble) inSingle = !inSingle;
    else if (c === '"' && !inSingle) inDouble = !inDouble;
  }
  return inSingle || inDouble;
}

function parseScalar(s: string, raw: string): YamlValue {
  if (s === "true") return true;
  if (s === "false") return false;
  if (s === "null" || s === "~") return null;
  if (s === "[]") return [];
  if (s === "{}") return {};
  if (s.startsWith("[") && s.endsWith("]")) {
    const inner = s.slice(1, -1).trim();
    if (!inner) return [];
    return inner
      .split(",")
      .map((part) => parseScalar(part.trim(), raw))
      .filter((item) => item !== "" && item != null);
  }
  if (s.startsWith("{")) {
    throw new Error(`Nested maps are not supported: ${raw}`);
  }
  if (s.startsWith("&") || s.startsWith("*")) {
    throw new Error(`YAML anchors are not supported: ${raw}`);
  }
  if (s.startsWith("|") || s.startsWith(">")) {
    throw new Error(`Block scalars are not supported: ${raw}`);
  }
  if (JSON_NUMBER.test(s)) return Number(s);
  return unquote(s);
}

function unquote(s: string): string {
  if (s.length >= 2 && s.startsWith('"') && s.endsWith('"')) {
    return s.slice(1, -1).replace(/\\"/g, '"').replace(/\\n/g, "\n");
  }
  if (s.length >= 2 && s.startsWith("'") && s.endsWith("'")) {
    return s.slice(1, -1).replace(/''/g, "'");
  }
  return s;
}
