// Dev-only stand-in for the engine so the screens can be checked in a browser (`npm run dev`).
// VITE_MOCK points at a session.json on this PC. Never part of the app build.
let cache: any
async function session() {
  cache ??= await fetch(String(import.meta.env.VITE_MOCK)).then((r) => r.json())
  return cache
}
export async function mock(cmd: string, _args: Record<string, unknown>): Promise<any> {
  const s = await session()
  const rec = new URLSearchParams(location.search).has('rec')
  switch (cmd) {
    case 'status':
      return { ffmpeg: true, claude: true, roblox: true, recording: rec ? { id: s.id, index: s.current, seconds: 83 } : null, vault: 'vault' }
    case 'session':
      return { session: s, dir: '' }
    case 'sessions':
      return [{ id: s.id, created: s.created, game: s.game.name, icon: s.game.icon, status: s.status, step: '', segments: s.plan.length, recorded: 2, teardown: s.teardown }]
    case 'teardown':
      return { markdown: `# ${s.game.name}\n## Summary\nSample analysis text for checking the layout.\n## Core loop\nThrow food → animals grow → earn cash → upgrade.`, path: 'C:/vault/Teardowns/x.md' }
    case 'search':
      return [s.game]
    default:
      return { ok: true }
  }
}
