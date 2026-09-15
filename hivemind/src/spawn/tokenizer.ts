export type TokenizeResult = { kind: "ok"; argv: string[] } | { kind: "fail" };

export function tokenize(cmd: string): TokenizeResult {
  const argv: string[] = [];
  let i = 0;
  while (i < cmd.length) {
    while (i < cmd.length && isSpace(cmd[i])) i += 1;
    if (i >= cmd.length) break;
    const quote = cmd[i];
    if (quote === '"' || quote === "'") {
      i += 1;
      let token = "";
      let closed = false;
      while (i < cmd.length) {
        if (cmd[i] === quote) {
          closed = true;
          i += 1;
          break;
        }
        token += cmd[i];
        i += 1;
      }
      if (!closed) return { kind: "fail" };
      argv.push(token);
      continue;
    }
    let token = "";
    while (i < cmd.length && !isSpace(cmd[i])) {
      token += cmd[i];
      i += 1;
    }
    argv.push(token);
  }
  return { kind: "ok", argv };
}

function isSpace(ch: string): boolean {
  return ch === " " || ch === "\t" || ch === "\n" || ch === "\r";
}
