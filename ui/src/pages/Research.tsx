import { ArrowDown, ArrowUp, Brain, Library, Search, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { GameRow, Status, ago, fmt, useApi } from '../lib/api'
import { Card, Empty, GameIcon, MomentumBar, PageHeader, Skeleton, SourceBadge, cx } from '../components/ui'
import { go } from '../App'

type SortKey = 'rank' | 'playing' | 'like_ratio' | 'momentum' | 'fun' | 'last_analysis'
const FILTERS = [
  { id: 'all', label: 'All' },
  { id: 'learned', label: 'Learned' },
  { id: 'videos', label: 'From videos' },
  { id: 'data', label: 'Data + AI' },
  { id: 'none', label: 'Not learned' },
]

export default function Research({ status }: { status: Status }) {
  const { data } = useApi<GameRow[]>('games', {}, 20000)
  const [q, setQ] = useState('')
  const [filter, setFilter] = useState('all')
  const [genre, setGenre] = useState('')
  const [sort, setSort] = useState<{ k: SortKey; desc: boolean }>({ k: 'rank', desc: false })

  const genres = useMemo(() => [...new Set((data ?? []).map((g) => g.genre).filter(Boolean))].sort(), [data])
  const counts = useMemo(() => {
    const d = data ?? []
    return { all: d.length, learned: d.filter((g) => g.source).length, videos: d.filter((g) => g.source === 'videos').length, data: d.filter((g) => g.source === 'data').length, none: d.filter((g) => !g.source).length } as Record<string, number>
  }, [data])

  const rows = useMemo(() => {
    let r = (data ?? []).filter((g) => {
      if (q && !(g.title + ' ' + g.genre + ' ' + g.one_liner).toLowerCase().includes(q.toLowerCase())) return false
      if (genre && g.genre !== genre) return false
      if (filter === 'learned') return !!g.source
      if (filter === 'none') return !g.source
      if (filter !== 'all') return g.source === filter
      return true
    })
    const val = (g: GameRow): number | string => {
      if (sort.k === 'rank') return g.rank ?? 9999
      if (sort.k === 'last_analysis') return g.last_analysis || ''
      return (g[sort.k] as number | null) ?? -1
    }
    r = [...r].sort((a, b) => {
      const x = val(a)
      const y = val(b)
      const c = x < y ? -1 : x > y ? 1 : 0
      return sort.desc ? -c : c
    })
    return r
  }, [data, q, filter, genre, sort])

  const head = (k: SortKey, label: string, cls = '') => (
    <th className={cx('px-3 py-2.5 font-semibold', cls)}>
      <button
        className={cx('inline-flex items-center gap-1 uppercase tracking-wider hover:text-ink-2', sort.k === k && 'text-ink-2')}
        onClick={() => setSort((s) => ({ k, desc: s.k === k ? !s.desc : k !== 'rank' }))}
      >
        {label}
        {sort.k === k && (sort.desc ? <ArrowDown className="size-3" /> : <ArrowUp className="size-3" />)}
      </button>
    </th>
  )

  return (
    <>
      <PageHeader
        title="Research"
        subtitle={`${counts.learned ?? 0} of ${counts.all ?? 0} games learned · ${status.mode === 'active' ? 'learning from YouTube videos' : 'Passive mode: data only'}`}
      />

      <div className="mb-4 flex flex-wrap items-center gap-3">
        <div className="relative w-72">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-mute" />
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search games, genres…"
            className="h-10 w-full rounded-[10px] border border-line bg-card pl-9 pr-8 text-[13.5px] text-ink outline-none placeholder:text-mute focus:border-brand/60"
          />
          {q && (
            <button onClick={() => setQ('')} className="absolute right-2.5 top-1/2 -translate-y-1/2 text-mute hover:text-ink">
              <X className="size-4" />
            </button>
          )}
        </div>
        <div className="flex rounded-[10px] border border-line bg-card p-1">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              onClick={() => setFilter(f.id)}
              className={cx('flex h-8 items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-semibold', filter === f.id ? 'bg-card-2 text-ink ring-1 ring-line-2' : 'text-mute hover:text-ink-2')}
            >
              {f.label}
              <span className="text-[11px] font-medium text-mute">{counts[f.id] ?? 0}</span>
            </button>
          ))}
        </div>
        <select
          value={genre}
          onChange={(e) => setGenre(e.target.value)}
          className="h-10 rounded-[10px] border border-line bg-card px-3 text-[13px] text-ink-2 outline-none focus:border-brand/60"
        >
          <option value="">All genres</option>
          {genres.map((g) => (
            <option key={g}>{g}</option>
          ))}
        </select>
      </div>

      {!data ? (
        <div className="space-y-2">{[...Array(8)].map((_, i) => <Skeleton key={i} className="h-14" />)}</div>
      ) : data.length === 0 ? (
        <Empty icon={<Library className="size-6" />} title="No games yet" text="Games appear here after the first chart update, usually within a minute of opening the app." />
      ) : rows.length === 0 ? (
        <Empty icon={<Search className="size-6" />} title="No matches" text="Try a different search or filter." />
      ) : (
        <Card className="overflow-hidden">
          <table className="w-full text-[13px]">
            <thead className="border-b border-line bg-card-2/50 text-left text-[11px] text-mute">
              <tr>
                {head('rank', '#', 'w-14 text-right')}
                <th className="px-3 py-2.5 font-semibold uppercase tracking-wider">Game</th>
                {head('playing', 'Playing')}
                {head('like_ratio', 'Likes')}
                {head('momentum', 'Momentum')}
                {head('fun', 'Fun')}
                <th className="px-3 py-2.5 font-semibold uppercase tracking-wider">Learned from</th>
                {head('last_analysis', 'Updated')}
              </tr>
            </thead>
            <tbody className="divide-y divide-line">
              {rows.map((g) => (
                <tr key={g.uid} onClick={() => go('game', String(g.uid))} className="cursor-pointer transition-colors hover:bg-card-2/60">
                  <td className="px-3 py-2.5 text-right tabular-nums text-mute">{g.rank ?? '–'}</td>
                  <td className="px-3 py-2.5">
                    <div className="flex items-center gap-3">
                      <GameIcon uid={g.uid} title={g.title} size={36} />
                      <div className="min-w-0">
                        <div className="max-w-[300px] truncate font-semibold text-ink">{g.title}</div>
                        <div className="max-w-[300px] truncate text-[12px] text-mute">{g.one_liner || g.genre || '—'}</div>
                      </div>
                    </div>
                  </td>
                  <td className="px-3 py-2.5 tabular-nums text-ink-2">{fmt(g.playing)}</td>
                  <td className="px-3 py-2.5 tabular-nums text-ink-2">{g.like_ratio ? `${Math.round(g.like_ratio)}%` : '–'}</td>
                  <td className="px-3 py-2.5">
                    <MomentumBar value={g.momentum} />
                  </td>
                  <td className="px-3 py-2.5 tabular-nums">{g.fun !== null ? <span className="font-semibold text-ink">{g.fun}<span className="text-mute">/10</span></span> : <span className="text-mute">–</span>}</td>
                  <td className="px-3 py-2.5">
                    <SourceBadge source={g.source} label={g.source_label} />
                  </td>
                  <td className="px-3 py-2.5 text-[12px] text-mute">{g.last_analysis ? ago(g.last_analysis) : '–'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
      {data && status.mode !== 'active' && counts.none > 0 && (
        <div className="mt-4 flex items-center gap-2 text-[12.5px] text-mute">
          <Brain className="size-4" /> Switch to Active mode on the Studio page to learn the rest from YouTube videos.
        </div>
      )}
    </>
  )
}
