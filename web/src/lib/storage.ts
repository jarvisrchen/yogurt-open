export function readNumber(key: string): number | null {
  try {
    const v = Number(localStorage.getItem(key));
    return Number.isFinite(v) && v > 0 ? v : null;
  } catch {
    return null;
  }
}

export function writeNumber(key: string, v: number) {
  try {
    localStorage.setItem(key, String(v));
  } catch {
    // Private mode or blocked storage: the choice lasts until reload.
  }
}

export function readString(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** `null` removes the key. */
export function writeString(key: string, v: string | null) {
  try {
    if (v == null) localStorage.removeItem(key);
    else localStorage.setItem(key, v);
  } catch {
    // Private mode or blocked storage: the choice lasts until reload.
  }
}
