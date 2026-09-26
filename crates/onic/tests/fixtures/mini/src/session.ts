export interface Saver {
  save(): void;
}

export class SessionStore {
  constructor() {}

  #probe() {}

  static {
    class Inner {
      encode() {}
    }
  }

  save(id: string): void {
    void id;
  }

  get(id: string): string | null {
    void id;
    return null;
  }
}

export function expireSessions(): void {}
void (() => {});
export function importedFn(): void {}
