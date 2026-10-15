/**
 * Production stand-in for `mock.ts`.
 *
 * `vite.config.ts` aliases `@fixtures` here for release builds, so the
 * development fixtures — and the invented balances in them — never reach a
 * shipped binary. Nothing calls this: `ipc.ts` only reaches for `@fixtures`
 * when running outside Tauri under `pnpm dev`.
 */
export function mockInvoke<T>(cmd: string): T {
  throw new Error(`development fixtures are unavailable in a release build (${cmd})`);
}
