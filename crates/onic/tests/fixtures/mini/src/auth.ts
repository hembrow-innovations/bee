import { SessionStore, importedFn } from "./session.js";

// @onic kind=why relates=login
// Sessions live on the server so they can be revoked.
export function login(user: string): string {
  const store = new SessionStore();
  store.save(user);
  expireSessions();
  return user;
}

export function runComputedSave(user: string): void {
  const store = new SessionStore();
  store["save"](user);
}

export function runNestedCall(): void {
  // @ts-expect-error nested-call fixture; factory stays unresolved
  factory()();
}

export const api = { hash() {} };

export function runInstance(password: string) {
  // @ts-expect-error instance-call fixture; PasswordHasher stays unresolved
  const hasher = new PasswordHasher();
  return hasher(password);
}

// @ts-expect-error instance-call fixture; PasswordHasher stays unresolved
const hasher = new PasswordHasher();
export function runModuleInstance(password: string) {
  return hasher(password);
}

export class HasherBox {
  // @ts-expect-error instance-call fixture; PasswordHasher stays unresolved
  box = new PasswordHasher();
  runClassInstance(password: string) {
    // @ts-expect-error instance-call fixture; box stays unresolved
    return box(password);
  }
}

// @ts-expect-error instance-call fixture; box stays unresolved
void box();

export function runImported() {
  importedFn();
}

export function runImportedMiss() {
  // @ts-expect-error unimported unique-name fixture
  missingImported();
}
