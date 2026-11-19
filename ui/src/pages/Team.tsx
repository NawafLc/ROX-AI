import { ArrowRight, Crown, Send } from 'lucide-react'
import { Home, useApi } from '../lib/api'
import { Card, GameIcon, PageHeader, Skeleton } from '../components/ui'
import { AgentCard } from '../components/Team'
import { go } from '../App'

/** The AI team: talk to the Lead, see what everyone else is doing. */
export default function Team() {
  const { data: h } = useApi<Home>('home', {}, 3000)
  return (
    <>
      <PageHeader title="Team" subtitle="You talk to the Lead. Everyone else just reports what they're doing." />

      <Card className="mb-5 p-5">
        <div className="flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl bg-card-2 text-mute">
            <Crown className="size-5" />
          </div>
          <div className="flex-1">
            <div className="text-[14.5px] font-semibold">Chat with the Lead</div>
            <div className="text-[12.5px] text-mute">The Lead joins when game building arrives. You'll give it direction and feedback here.</div>
          </div>
        </div>
        <div className="mt-4 flex items-center gap-2 rounded-xl border border-line bg-bg px-4 py-3 opacity-60">
          <span className="flex-1 text-[13.5px] text-mute">Message the Lead…</span>
          <Send className="size-4 text-mute" />
        </div>
      </Card>

      {!h ? (
        <div className="grid grid-cols-3 gap-4">{[...Array(6)].map((_, i) => <Skeleton key={i} className="h-32" />)}</div>
      ) : (
        <div className="grid grid-cols-3 gap-4">
          {h.agents.map((a) => (
            <AgentCard key={a.id} a={a} />
          ))}
        </div>
      )}

      {h && h.next.length > 0 && (
        <Card className="mt-5 p-5">
          <div className="text-[14.5px] font-semibold">Researcher's queue</div>
          <div className="text-[12.5px] text-mute">The next chart games it will learn.</div>
          <div className="mt-2 divide-y divide-line">
            {h.next.map((q) => (
              <button key={q.uid} onClick={() => go('game', String(q.uid))} className="flex w-full items-center gap-3 py-2.5 text-left hover:opacity-90">
                <span className="w-7 text-right text-[12px] tabular-nums text-mute">#{q.rank}</span>
                <GameIcon uid={q.uid} title={q.title} size={32} />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13.5px] font-medium">{q.title}</div>
                  <div className="text-[12px] text-mute">{q.reason}</div>
                </div>
                <ArrowRight className="size-4 text-mute" />
              </button>
            ))}
          </div>
        </Card>
      )}
    </>
  )
}
