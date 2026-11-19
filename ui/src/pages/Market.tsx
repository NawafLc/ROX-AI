import { ArrowDownRight, ArrowUpRight, Flame, LineChart, Minus, Sparkles } from 'lucide-react'
import { fmt, useApi } from '../lib/api'
import { Badge, Card, Empty, GameIcon, MomentumBar, PageHeader, Skeleton, SourceBadge, cx } from '../components/ui'
import { go } from '../App'

type Row = {
  uid: number
  rank: number
  title: string
  genre: string
  playing: number
  like_ratio: number
  momentum: number
  rank_change: number | null
  ccu_change: number | null
  flags: string[]
  days: number
  learned: string | null
}
type Chart = { day: string | null; chart?: string; rows: Row[]; genres?: { genre: string; games: number; playing: number }[] }

const GENRE_COLORS = ['#00d6a8', '#6ea0ff', '#ffc452', '#f05260', '#b388ff', '#4dd0e1', '#ff8a65', '#9ccc65']

export default function Market() {
  const { data } = useApi<Chart>('chart', {}, 30000)
  if (!data)
    return (
      <>
        <PageHeader title="Market" />
        <Skeleton className="h-40" />
        <Skeleton className="mt-4 h-96" />
      </>
    )
  if (!data.day) return <Empty icon={<LineChart className="size-6" />} title="No chart yet" text="The first chart update runs automatically. Check back in a minute." />

  const rows = data.rows.map((r) => ({ ...r, title: r.title || 'New game (loading…)' }))
  const total = rows.reduce((a, r) => a + r.playing, 0)
  const newOnes = rows.filter((r) => r.flags.includes('new-entry') || r.days <= 1)
  const movers = [...rows].filter((r) => r.rank_change !== null).sort((a, b) => (b.rank_change ?? 0) - (a.rank_change ?? 0))
  const hot = [...rows].sort((a, b) => b.momentum - a.momentum || b.playing - a.playing).slice(0, 5)
  const genres = data.genres ?? []
  const gTotal = genres.reduce((a, g) => a + g.playing, 0) || 1

  return (
    <>
      <PageHeader title="Market" subtitle={`${data.chart} · chart of ${data.day} · ${fmt(total)} players across the top ${rows.length}`} />

      <div className="grid grid-cols-3 gap-4">
        <Card className="col-span-2 p-5">
          <div className="text-[14px] font-semibold">Where players are</div>
          <div className="mt-0.5 text-[12.5px] text-mute">Share of players on today's chart, by genre</div>
          <div className="mt-4 flex h-3 overflow-hidden rounded-full">
            {genres.map((g, i) => (
              <div key={g.genre} title={`${g.genre}: ${fmt(g.playing)}`} style={{ width: `${(g.playing / gTotal) * 100}%`, background: GENRE_COLORS[i % GENRE_COLORS.length] }} />
            ))}
          </div>
          <div className="mt-4 grid grid-cols-2 gap-x-6 gap-y-2">
            {genres.slice(0, 8).map((g, i) => (
              <div key={g.genre} className="flex items-center gap-2.5 text-[13px]">
                <span className="size-2.5 rounded-sm" style={{ background: GENRE_COLORS[i % GENRE_COLORS.length] }} />
                <span className="flex-1 truncate text-ink-2">{g.genre || 'Other'}</span>
                <span className="tabular-nums text-mute">{g.games} games</span>
                <span className="w-14 text-right font-semibold tabular-nums">{Math.round((g.playing / gTotal) * 100)}%</span>
              </div>
            ))}
          </div>
        </Card>
        <Card className="p-5">
          <div className="flex items-center gap-2 text-[14px] font-semibold">
            <Flame className="size-4 text-amber" /> Hottest right now
          </div>
          <div className="mt-3 space-y-1">
            {hot.map((r) => (
              <button key={r.uid} onClick={() => go('game', String(r.uid))} className="flex w-full items-center gap-3 rounded-lg px-1 py-1.5 text-left hover:bg-card-2">
                <GameIcon uid={r.uid} title={r.title} size={30} />
                <span className="min-w-0 flex-1 truncate text-[13px] font-medium">{r.title}</span>
                <MomentumBar value={r.momentum} />
              </button>
            ))}
          </div>
          {newOnes.length > 0 && (
            <div className="mt-4 flex items-center gap-2 rounded-lg bg-card-2 px-3 py-2 text-[12.5px] text-ink-2">
              <Sparkles className="size-4 text-brand" /> {newOnes.length} new {newOnes.length === 1 ? 'game' : 'games'} on the chart today
            </div>
          )}
        </Card>
      </div>

      <Card className="mt-4 overflow-hidden">
        <div className="flex items-center justify-between border-b border-line px-5 py-3.5">
          <div className="text-[14px] font-semibold">Today's chart</div>
          {movers.length > 0 && movers[0].rank_change! > 0 && (
            <div className="text-[12.5px] text-mute">
              Biggest climb: <span className="font-semibold text-brand">{movers[0].title}</span> (+{movers[0].rank_change})
            </div>
          )}
        </div>
        <table className="w-full text-[13px]">
          <thead className="bg-card-2/50 text-left text-[11px] uppercase tracking-wider text-mute">
            <tr>
              <th className="w-14 px-3 py-2.5 text-right font-semibold">#</th>
              <th className="w-14 px-1 py-2.5 font-semibold">Move</th>
              <th className="px-3 py-2.5 font-semibold">Game</th>
              <th className="px-3 py-2.5 font-semibold">Genre</th>
              <th className="px-3 py-2.5 font-semibold">Playing</th>
              <th className="px-3 py-2.5 font-semibold">Likes</th>
              <th className="px-3 py-2.5 font-semibold">Momentum</th>
              <th className="px-3 py-2.5 font-semibold">Learned</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line">
            {rows.map((r) => (
              <tr key={r.uid} onClick={() => go('game', String(r.uid))} className="cursor-pointer hover:bg-card-2/60">
                <td className="px-3 py-2.5 text-right font-semibold tabular-nums">{r.rank}</td>
                <td className="px-1 py-2.5">
                  <Move r={r} />
                </td>
                <td className="px-3 py-2.5">
                  <div className="flex items-center gap-3">
                    <GameIcon uid={r.uid} title={r.title} size={32} />
                    <span className="max-w-[280px] truncate font-semibold">{r.title}</span>
                  </div>
                </td>
                <td className="px-3 py-2.5 text-ink-2">{r.genre || '–'}</td>
                <td className="px-3 py-2.5 tabular-nums text-ink-2">
                  {fmt(r.playing)}
                  {r.ccu_change !== null && <span className={cx('ml-1.5 text-[11.5px]', r.ccu_change >= 0 ? 'text-brand' : 'text-rose')}>{r.ccu_change >= 0 ? '+' : ''}{Math.round(r.ccu_change)}%</span>}
                </td>
                <td className="px-3 py-2.5 tabular-nums text-ink-2">{Math.round(r.like_ratio)}%</td>
                <td className="px-3 py-2.5">
                  <MomentumBar value={r.momentum} />
                </td>
                <td className="px-3 py-2.5">{r.learned ? <SourceBadge source={r.learned} /> : <span className="text-[12px] text-mute">–</span>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </>
  )
}

function Move({ r }: { r: Row }) {
  if (r.days <= 1 || r.rank_change === null) return <Badge tone="brand">New</Badge>
  if (r.rank_change > 0)
    return (
      <span className="inline-flex items-center text-[12px] font-semibold text-brand">
        <ArrowUpRight className="size-3.5" />
        {r.rank_change}
      </span>
    )
  if (r.rank_change < 0)
    return (
      <span className="inline-flex items-center text-[12px] font-semibold text-rose">
        <ArrowDownRight className="size-3.5" />
        {-r.rank_change}
      </span>
    )
  return <Minus className="size-3.5 text-mute" />
}
