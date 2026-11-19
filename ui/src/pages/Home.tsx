import { ArrowRight, CalendarCheck, Gamepad2, Hammer, Lightbulb, MessageSquare, Play, ShieldCheck, Upload, Users } from 'lucide-react'
import { Home as HomeData, MyGame, Status, useApi } from '../lib/api'
import { Button, Card, Skeleton, cx } from '../components/ui'
import { AgentRow } from '../components/Team'
import { GameCard } from '../components/GameCard'
import { go } from '../App'

export default function Home({ status }: { status: Status }) {
  const { data: h } = useApi<HomeData>('home', {}, 3000)
  const { data: games } = useApi<MyGame[]>('my_games', {}, 5000)
  const hour = new Date().getHours()
  const hello = hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening'
  const working = h?.working ?? 0

  return (
    <>
      <div className="mb-6 flex items-end justify-between gap-4">
        <div>
          <h1 className="text-[26px] font-bold tracking-tight">{hello}</h1>
          <div className="mt-1 text-[13.5px] text-mute">{games && games.length ? `${games.length} ${games.length === 1 ? 'game' : 'games'} in your studio` : 'Your studio'}</div>
        </div>
        <button onClick={() => go('team')} className="flex items-center gap-2 rounded-full border border-line bg-card px-3.5 py-1.5 text-[12.5px] font-medium text-ink-2 hover:border-line-2 hover:text-ink">
          <span className={cx('size-2 rounded-full', working ? 'pulse-dot bg-brand text-brand' : 'bg-sky')} />
          {working ? `${working} of your team ${working === 1 ? 'is' : 'are'} working` : 'Team is standing by'}
        </button>
      </div>

      {/* ---------- your games ---------- */}
      <div className="mb-3 flex items-center justify-between">
        <h2 className="text-[16px] font-semibold">Your games</h2>
        {games && games.length > 0 && (
          <button onClick={() => go('games')} className="flex items-center gap-1 text-[12.5px] font-medium text-brand hover:underline">
            See all <ArrowRight className="size-3.5" />
          </button>
        )}
      </div>
      {!games ? (
        <div className="grid grid-cols-3 gap-4">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-64" />)}</div>
      ) : games.length === 0 ? (
        <FirstGame />
      ) : (
        <div className="grid grid-cols-3 gap-4">
          {games.slice(0, 6).map((g) => (
            <GameCard key={g.id} g={g} />
          ))}
        </div>
      )}

      {/* ---------- team + today ---------- */}
      <div className="mt-6 grid grid-cols-3 gap-5">
        <Card className="col-span-2 p-5">
          <div className="mb-1 flex items-center justify-between">
            <div className="flex items-center gap-2 text-[15px] font-semibold">
              <Users className="size-4 text-mute" /> Your team
            </div>
            <button onClick={() => go('team')} className="text-[12.5px] font-medium text-brand hover:underline">
              Open team
            </button>
          </div>
          {!h ? (
            <div className="mt-3 grid grid-cols-2 gap-3">{[...Array(6)].map((_, i) => <Skeleton key={i} className="h-11" />)}</div>
          ) : (
            <div className="grid grid-cols-2 gap-x-6">
              {h.agents.map((a) => (
                <AgentRow key={a.id} a={a} />
              ))}
            </div>
          )}
        </Card>

        <Card className="p-5">
          <div className="flex items-center gap-2 text-[15px] font-semibold">
            <CalendarCheck className="size-4 text-mute" /> Today
          </div>
          {!h ? (
            <Skeleton className="mt-3 h-20" />
          ) : (
            <ul className="mt-3 space-y-2.5">
              {h.summary.map((s, i) => (
                <li key={i} className="flex items-center gap-2.5 text-[13.5px] text-ink-2">
                  <span className="size-1.5 shrink-0 rounded-full bg-brand" />
                  {s[0].toUpperCase() + s.slice(1)}
                </li>
              ))}
            </ul>
          )}
          {status.blocked && (
            <button onClick={() => go('research')} className="mt-4 w-full rounded-lg border border-amber/30 bg-amber/8 px-3 py-2 text-left text-[12.5px] text-amber">
              Needs you: {status.blocked}
            </button>
          )}
        </Card>
      </div>
    </>
  )
}

/** Empty state for a brand-new studio: what will happen to every game. */
export function FirstGame() {
  const steps = [
    { icon: Lightbulb, label: 'Idea' },
    { icon: Hammer, label: 'Build' },
    { icon: ShieldCheck, label: 'Test' },
    { icon: Play, label: 'You play' },
    { icon: MessageSquare, label: 'Feedback' },
    { icon: Upload, label: 'Publish' },
  ]
  return (
    <Card className="relative overflow-hidden p-8">
      <div className="pointer-events-none absolute -right-20 -top-28 size-80 rounded-full bg-brand/10 blur-3xl" />
      <div className="pointer-events-none absolute -bottom-32 left-1/3 size-72 rounded-full bg-sky/8 blur-3xl" />
      <div className="relative flex items-center gap-10">
        <div className="grid size-24 shrink-0 place-items-center rounded-3xl bg-gradient-to-br from-brand/25 to-sky/10 text-brand ring-1 ring-brand/30">
          <Gamepad2 className="size-11" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-[20px] font-bold tracking-tight">Your first game is on its way</div>
          <div className="mt-1.5 max-w-xl text-[13.5px] leading-relaxed text-ink-2">
            Your AI team builds games here on its own. Each one shows up ready for you to play, give feedback and publish. Press Start building on the Team page and the Lead picks the first game from the market.
          </div>
          <Button kind="primary" className="mt-4" icon={<Play className="size-4" />} onClick={() => go('team')}>
            Open the team
          </Button>
          <div className="mt-5 flex flex-wrap items-center gap-1.5">
            {steps.map((s, i) => (
              <div key={s.label} className="flex items-center gap-1.5">
                <span className="flex items-center gap-1.5 rounded-full border border-line-2 bg-card-2/80 px-3 py-1 text-[12px] font-medium text-ink-2">
                  <s.icon className="size-3.5 text-brand" />
                  {s.label}
                </span>
                {i < steps.length - 1 && <ArrowRight className="size-3 text-mute" />}
              </div>
            ))}
          </div>
        </div>
      </div>
    </Card>
  )
}
