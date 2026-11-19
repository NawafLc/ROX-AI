import { BookOpen, ChevronRight, ExternalLink, FileText, FolderOpen, Search } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Note, call, useApi } from '../lib/api'
import { Button, Card, Empty, Skeleton, cx } from '../components/ui'
import { Markdown } from '../components/Markdown'
import { go } from '../App'

type Folder = { folder: string; files: string[] }
const SHOW_PROPS = ['genre', 'chart_rank', 'best_rank', 'playing', 'like_ratio', 'momentum', 'fun_score', 'analysis_source', 'last_scan', 'creator']
const LABELS: Record<string, string> = { chart_rank: 'Rank', best_rank: 'Best rank', like_ratio: 'Likes %', fun_score: 'Fun', analysis_source: 'Learned from', last_scan: 'Updated' }

export default function Vault({ path }: { path: string }) {
  const { data: tree } = useApi<Folder[]>('vault_tree')
  const current = path || '00 Dashboard.md'
  const { data: note, error } = useApi<Note>('note', { path: current })
  const [q, setQ] = useState('')
  const [closed, setClosed] = useState<Record<string, boolean>>({ Charts: true, Insights: true, Playtests: true })

  const shown = useMemo(
    () => (tree ?? []).map((f) => ({ ...f, files: f.files.filter((p) => !q || name(p).toLowerCase().includes(q.toLowerCase())) })).filter((f) => f.files.length || !q),
    [tree, q],
  )
  const link = (t: string) => call<{ path: string | null }>('resolve', { target: t }).then((r) => r.path && go('vault', r.path))

  return (
    <div className="flex h-full">
      <div className="flex w-[260px] shrink-0 flex-col border-r border-line py-7 pr-4">
        <div className="mb-3 flex items-center justify-between">
          <h1 className="text-[20px] font-bold tracking-tight">Vault</h1>
          <Button size="sm" kind="ghost" icon={<ExternalLink className="size-3.5" />} onClick={() => call('open', { what: 'obsidian' })}>
            Obsidian
          </Button>
        </div>
        <div className="relative mb-3">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-mute" />
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Find a note"
            className="h-9 w-full rounded-lg border border-line bg-card pl-8 pr-3 text-[13px] outline-none placeholder:text-mute focus:border-brand/60"
          />
        </div>
        <div className="-mr-2 flex-1 overflow-y-auto pr-2">
          {!tree ? (
            <div className="space-y-2">{[...Array(8)].map((_, i) => <Skeleton key={i} className="h-7" />)}</div>
          ) : (
            shown.map((f) =>
              f.folder === 'Dashboard' ? (
                <Item key="dash" active={current === f.files[0]} onClick={() => go('vault', f.files[0])} icon={<BookOpen className="size-4" />} label="Dashboard" />
              ) : (
                <div key={f.folder} className="mt-2">
                  <button
                    onClick={() => setClosed((c) => ({ ...c, [f.folder]: !c[f.folder] }))}
                    className="flex h-8 w-full items-center gap-1.5 rounded-md px-1.5 text-[11.5px] font-semibold uppercase tracking-wider text-mute hover:text-ink-2"
                  >
                    <ChevronRight className={cx('size-3.5 transition-transform', !(closed[f.folder] && !q) && 'rotate-90')} />
                    {f.folder}
                    <span className="ml-auto text-[11px] font-medium normal-case tracking-normal">{f.files.length}</span>
                  </button>
                  {!(closed[f.folder] && !q) &&
                    (f.files.length === 0 ? (
                      <div className="px-8 py-1 text-[12px] text-mute">Empty</div>
                    ) : (
                      f.files.map((p) => <Item key={p} active={current === p} onClick={() => go('vault', p)} icon={<FileText className="size-3.5" />} label={name(p)} indent />)
                    ))}
                </div>
              ),
            )
          )}
        </div>
      </div>

      <div className="min-w-0 flex-1 overflow-y-auto py-7 pl-8 pr-8">
        {error ? (
          <Empty icon={<FolderOpen className="size-6" />} title="Note not found" text="It may have been renamed or deleted." />
        ) : !note ? (
          <div className="space-y-3">
            <Skeleton className="h-10 w-2/3" />
            <Skeleton className="h-64" />
          </div>
        ) : (
          <div key={note.path} className="fade-in max-w-[820px]">
            <div className="mb-3 flex items-center gap-2 text-[12px] text-mute">
              <span>{note.path.includes('/') ? note.path.split('/')[0].replace(/^\d+ /, '') : 'Vault'}</span>
              <ChevronRight className="size-3" />
              <span className="text-ink-2">{note.title.replace(/^\d+ /, '')}</span>
              <button onClick={() => call('open', { what: 'note', path: note.path })} className="ml-auto font-medium text-brand hover:underline">
                Open in Obsidian
              </button>
            </div>
            <Props props={note.props} />
            <Markdown body={note.body} onLink={link} />
          </div>
        )}
      </div>
    </div>
  )
}

function name(p: string) {
  return p.split('/').pop()!.replace(/\.md$/, '')
}

function Item({ active, onClick, icon, label, indent }: { active: boolean; onClick: () => void; icon: React.ReactNode; label: string; indent?: boolean }) {
  return (
    <button
      onClick={onClick}
      title={label}
      className={cx('flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[13px]', indent && 'pl-6', active ? 'bg-card-2 text-ink' : 'text-ink-2 hover:bg-card hover:text-ink')}
    >
      <span className={active ? 'text-brand' : 'text-mute'}>{icon}</span>
      <span className="truncate">{label}</span>
    </button>
  )
}

function Props({ props }: { props: Record<string, string> }) {
  const items = SHOW_PROPS.filter((k) => props[k] && props[k] !== '""')
  if (!items.length) return null
  return (
    <Card className="mb-5 flex flex-wrap gap-x-6 gap-y-2 px-4 py-3">
      {items.map((k) => (
        <div key={k}>
          <div className="text-[10.5px] font-semibold uppercase tracking-wider text-mute">{LABELS[k] ?? k.replace(/_/g, ' ')}</div>
          <div className="text-[13px] font-medium text-ink">{fmtProp(k, props[k])}</div>
        </div>
      ))}
    </Card>
  )
}

function fmtProp(k: string, v: string) {
  const n = Number(v)
  if (k === 'playing' && !isNaN(n)) return n >= 1000 ? `${(n / 1000).toFixed(1).replace(/\.0$/, '')}K` : v
  if ((k === 'chart_rank' || k === 'best_rank') && v) return `#${v}`
  if (k === 'analysis_source') return v === 'videos' ? 'YouTube videos' : v === 'data' ? 'Data + AI' : v
  return v
}
