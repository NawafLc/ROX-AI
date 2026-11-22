import { ArrowRight, Search, Users } from 'lucide-react'
import { useState } from 'react'
import { Button, Card, GameIcon, PageHeader, Skeleton } from '../components/ui'
import { call, compact, Game, go } from '../lib/api'

export function HomePage() {
  const [q, setQ] = useState('')
  const [results, setResults] = useState<Game[] | null>(null)
  const [searching, setSearching] = useState(false)
  const [starting, setStarting] = useState<number | null>(null)
  const [err, setErr] = useState('')

  const search = async () => {
    if (!q.trim()) return
    setSearching(true)
    setErr('')
    try {
      setResults(await call<Game[]>('search', { query: q }))
    } catch (e) {
      setErr(String((e as Error).message ?? e))
    } finally {
      setSearching(false)
    }
  }

  const study = async (g: Game) => {
    setStarting(g.universe_id)
    setErr('')
    try {
      const s = await call<{ id: string }>('session_new', { universe_id: g.universe_id })
      go(`#/s/${s.id}`)
    } catch (e) {
      setErr(String((e as Error).message ?? e))
      setStarting(null)
    }
  }

  return (
    <div className="fade-in">
      <PageHeader title="Study a game" subtitle="Pick a Roblox game. The AI plans what to film, guides you on screen while you play, then studies the videos with your notes." />

      <form
        onSubmit={(e) => {
          e.preventDefault()
          search()
        }}
        className="flex gap-2"
      >
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-3.5 top-1/2 size-4 -translate-y-1/2 text-mute" />
          <input
            autoFocus
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Game name, Roblox link or place id"
            className="selectable h-12 w-full rounded-xl border border-line-2 bg-card pl-10 pr-4 text-[14.5px] text-ink outline-none placeholder:text-mute focus:border-brand/60"
          />
        </div>
        <Button kind="primary" size="lg" type="submit" loading={searching} disabled={!q.trim()}>
          Search
        </Button>
      </form>
      {err && <div className="mt-3 text-[13px] text-rose">{err}</div>}

      <div className="mt-6">
        {searching ? (
          <div className="grid grid-cols-2 gap-3">
            {[0, 1, 2, 3].map((k) => (
              <Skeleton key={k} className="h-[88px]" />
            ))}
          </div>
        ) : results === null ? (
          <HowItWorks />
        ) : results.length === 0 ? (
          <div className="text-[13.5px] text-mute">No games found. Try the exact name or paste the game's link.</div>
        ) : (
          <div className="grid grid-cols-2 gap-3">
            {results.map((g) => (
              <Card key={g.universe_id} className="flex items-center gap-3.5 p-3.5 transition-colors hover:border-line-2">
                <GameIcon src={g.icon} title={g.name} size={60} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[14.5px] font-semibold text-ink">{g.name}</div>
                  <div className="truncate text-[12.5px] text-mute">by {g.creator}</div>
                  <div className="mt-1 flex items-center gap-3 text-[12px] text-ink-2">
                    <span className="flex items-center gap-1">
                      <Users className="size-3.5 text-brand" />
                      {compact(g.playing)} playing
                    </span>
                    <span className="text-mute">{compact(g.visits)} visits</span>
                  </div>
                </div>
                <Button kind="primary" size="sm" loading={starting === g.universe_id} disabled={starting !== null} onClick={() => study(g)} icon={<ArrowRight className="size-3.5" />}>
                  Study
                </Button>
              </Card>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function HowItWorks() {
  const steps = [
    ['Pick a game', 'Search by name or paste its link.'],
    ['The AI plans the shots', 'It looks the game up and writes a shot list: what to film, in order, and what each video must show.'],
    ['Film with the guide', 'A small guide floats over Roblox telling you what to do. Each segment is its own video. Type notes as you go.'],
    ['Push', 'Write your final notes and push. The AI studies every segment and writes a teardown for your new Playbook.'],
  ]
  return (
    <div className="grid grid-cols-4 gap-3">
      {steps.map(([t, d], k) => (
        <Card key={k} className="p-4">
          <div className="grid size-7 place-items-center rounded-lg bg-brand/12 text-[13px] font-bold text-brand">{k + 1}</div>
          <div className="mt-3 text-[14px] font-semibold text-ink">{t}</div>
          <div className="mt-1 text-[12.5px] leading-relaxed text-mute">{d}</div>
        </Card>
      ))}
    </div>
  )
}
