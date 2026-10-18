// FIXTURE-B CHANGE: b.ts no longer imports a.ts. The a<->b cycle is broken.
// (a.ts still imports stepB from here; the dependency is now one-way.)
export function stepB(depth: number): string {
  if (depth <= 0) return "b-done";
  return "b>" + stepB(depth - 1);
}
