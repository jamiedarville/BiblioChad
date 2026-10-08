// Things that must run before the window closes (e.g. saving the reading
// position). The Rust side waits ~1.5 s for us before closing anyway.

const flushers = new Set<() => Promise<void> | void>()

export function onBeforeClose(fn: () => Promise<void> | void): () => void {
  flushers.add(fn)
  return () => flushers.delete(fn)
}

export async function runBeforeClose(): Promise<void> {
  await Promise.allSettled([...flushers].map((f) => f()))
}
