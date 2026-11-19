import { BookOpen, Download, Gamepad2, LayoutDashboard, Library, LineChart, Loader2, RefreshCw, Settings as Cog, Sparkles } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { Status, call, useApi } from './lib/api'
import { Toast, cx } from './components/ui'
import { Logo } from './components/Logo'
import Studio from './pages/Studio'
import MyGames from './pages/MyGames'
import Research from './pages/Research'
import GameDetail from './pages/GameDetail'
import Market from './pages/Market'
import Vault from './pages/Vault'
import SettingsPage from './pages/Settings'
import Setup from './pages/Setup'

// ---------- tiny hash router: #/page/arg ----------
export type Route = { page: string; arg: string }
function parse(): Route {
  const h = decodeURIComponent(location.hash.replace(/^#\/?/, ''))
  const i = h.indexOf('/')
  return i < 0 ? { page: h || 'studio', arg: '' } : { page: h.slice(0, i), arg: h.slice(i + 1) }
}
export function go(page: string, arg = '') {
  location.hash = '/' + page + (arg ? '/' + encodeURIComponent(arg) : '')
}

const NAV = [
  { id: 'studio', label: 'Studio', icon: LayoutDashboard },
  { id: 'games', label: 'My Games', icon: Gamepad2 },
  { id: 'research', label: 'Research', icon: Library },
  { id: 'market', label: 'Market', icon: LineChart },
  { id: 'vault', label: 'Vault', icon: BookOpen },
]

export default function App() {
  const [route, setRoute] = useState<Route>(parse)
  useEffect(() => {
    const f = () => setRoute(parse())
    window.addEventListener('hashchange', f)
    return () => window.removeEventListener('hashchange', f)
  }, [])
  const { data: status, reload } = useApi<Status>('status', {}, 2000)
  const [toast, setToast] = useState<{ message: string; ok: boolean } | null>(null)
  const [lastNotice, setLastNotice] = useState('')
  useEffect(() => {
    const n = status?.notice
    if (n && n.message !== lastNotice) {
      setLastNotice(n.message)
      setToast(n)
    }
  }, [status?.notice, lastNotice])
  const closeToast = useCallback(() => setToast(null), [])

  if (!status)
    return (
      <div className="grid h-full place-items-center">
        <div className="flex flex-col items-center gap-4">
          <Logo size={44} />
          <Loader2 className="size-5 animate-spin text-mute" />
        </div>
      </div>
    )

  if (!status.setup_complete) return <Setup status={status} onDone={reload} />

  const active = route.page === 'game' ? 'research' : route.page
  let page
  switch (route.page) {
    case 'games':
      page = <MyGames />
      break
    case 'research':
      page = <Research status={status} />
      break
    case 'game':
      page = <GameDetail uid={Number(route.arg)} status={status} />
      break
    case 'market':
      page = <Market />
      break
    case 'vault':
      page = <Vault path={route.arg} />
      break
    case 'settings':
      page = <SettingsPage status={status} onChange={reload} />
      break
    default:
      page = <Studio status={status} onChange={reload} />
  }

  return (
    <div className="flex h-full">
      <aside className="flex w-[228px] shrink-0 flex-col border-r border-line bg-panel px-3 py-4">
        <div className="mb-6 flex items-center gap-2.5 px-2.5">
          <Logo size={30} />
          <div>
            <div className="text-[15px] font-bold tracking-tight">ROX-AI</div>
            <div className="text-[11px] text-mute">Game studio</div>
          </div>
        </div>
        <nav className="flex flex-col gap-0.5">
          {NAV.map((n) => (
            <NavItem key={n.id} active={active === n.id} onClick={() => go(n.id)} icon={<n.icon className="size-[18px]" />} label={n.label} badge={n.id === 'studio' && status.busy} />
          ))}
        </nav>
        <div className="mt-auto flex flex-col gap-2">
          <UpdateCard status={status} />
          <NavItem active={active === 'settings'} onClick={() => go('settings')} icon={<Cog className="size-[18px]" />} label="Settings" />
          <div className="px-3 text-[11px] text-mute">Version {status.version}</div>
        </div>
      </aside>
      <main className={cx('min-w-0 flex-1', route.page === 'vault' ? 'overflow-hidden' : 'overflow-y-auto')}>
        <div key={route.page === 'vault' ? 'vault' : route.page + route.arg} className={route.page === 'vault' ? 'h-full pl-8' : 'fade-in mx-auto max-w-[1180px] px-8 py-7'}>
          {page}
        </div>
      </main>
      {toast && <Toast message={toast.message} ok={toast.ok} onClose={closeToast} />}
    </div>
  )
}

function NavItem({ active, onClick, icon, label, badge }: { active: boolean; onClick: () => void; icon: React.ReactNode; label: string; badge?: boolean }) {
  return (
    <button
      onClick={onClick}
      className={cx(
        'group relative flex h-10 items-center gap-3 rounded-[10px] px-3 text-[13.5px] font-medium transition-colors',
        active ? 'bg-card-2 text-ink' : 'text-ink-2 hover:bg-card hover:text-ink',
      )}
    >
      {active && <span className="absolute left-0 top-2.5 h-5 w-[3px] rounded-r-full bg-brand" />}
      <span className={active ? 'text-brand' : 'text-mute group-hover:text-ink-2'}>{icon}</span>
      {label}
      {badge && <span className="pulse-dot ml-auto size-2 rounded-full bg-brand text-brand" />}
    </button>
  )
}

/** Always visible: tells you if there's a new version and installs it with one click. */
function UpdateCard({ status }: { status: Status }) {
  const u = status.update
  const [busy, setBusy] = useState(false)
  if (u.state === 'available' || u.state === 'installing')
    return (
      <div className="rounded-xl border border-brand/30 bg-brand/8 p-3">
        <div className="flex items-center gap-2 text-[12.5px] font-semibold text-brand">
          <Sparkles className="size-4" /> Update ready
        </div>
        <div className="mt-0.5 text-[11.5px] text-ink-2">Version {u.latest} is out.</div>
        <button
          disabled={u.state === 'installing' || busy}
          onClick={() => {
            setBusy(true)
            call('update_run')
          }}
          className="mt-2.5 flex h-8 w-full items-center justify-center gap-1.5 rounded-lg bg-brand text-[12.5px] font-semibold text-[#04140f] hover:brightness-110 disabled:opacity-60"
        >
          {u.state === 'installing' || busy ? <Loader2 className="size-3.5 animate-spin" /> : <Download className="size-3.5" />}
          {u.state === 'installing' || busy ? 'Installing…' : 'Install update'}
        </button>
      </div>
    )
  return (
    <button
      onClick={() => call('update_check')}
      className="flex h-9 items-center gap-2.5 rounded-[10px] px-3 text-[12.5px] text-mute hover:bg-card hover:text-ink-2"
      title={u.state === 'failed' ? u.error : undefined}
    >
      <RefreshCw className={cx('size-4', u.state === 'checking' && 'animate-spin')} />
      {u.state === 'checking' ? 'Checking for updates…' : u.state === 'failed' ? "Couldn't check — retry" : 'Up to date'}
    </button>
  )
}
