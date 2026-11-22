import '@fontsource-variable/inter'
import './styles.css'
import { BookOpen, Clapperboard, FolderOpen, GraduationCap, Search } from 'lucide-react'
import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TitleBar } from './components/TitleBar'
import { cx } from './components/ui'
import { call, Status, useApi } from './lib/api'
import { HomePage } from './pages/Home'
import { PlaybookPage } from './pages/Playbook'
import { SessionPage } from './pages/Session'
import { SessionsPage } from './pages/Sessions'
import { StudiedGame, StudiedPage } from './pages/Studied'

function useRoute() {
  const [r, setR] = useState(window.location.hash || '#/')
  useEffect(() => {
    const f = () => setR(window.location.hash || '#/')
    window.addEventListener('hashchange', f)
    return () => window.removeEventListener('hashchange', f)
  }, [])
  return r
}

function App() {
  const route = useRoute()
  const { data: st } = useApi<Status>('status', {}, 3000)
  const nav = [
    { r: '#/', label: 'Study a game', icon: <Search className="size-4" />, on: route === '#/' },
    { r: '#/sessions', label: 'Sessions', icon: <Clapperboard className="size-4" />, on: route === '#/sessions' || route.startsWith('#/s/') },
    { r: '#/studied', label: 'Studied', icon: <GraduationCap className="size-4" />, on: route.startsWith('#/studied') },
    { r: '#/playbook', label: 'Playbook', icon: <BookOpen className="size-4" />, on: route === '#/playbook' },
  ]
  const sid = route.startsWith('#/s/') ? decodeURIComponent(route.slice(4)) : ''
  const studied = route.startsWith('#/studied/') ? decodeURIComponent(route.slice(10)) : ''
  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <aside className="flex w-[228px] shrink-0 flex-col border-r border-line bg-panel px-3 pb-4 pt-3">
          <nav className="space-y-0.5">
            {nav.map((n) => (
              <a
                key={n.r}
                href={n.r}
                className={cx('flex h-9 items-center gap-2.5 rounded-lg px-3 text-[13.5px] font-medium transition-colors', n.on ? 'bg-card-2 text-ink' : 'text-ink-2 hover:bg-card hover:text-ink')}
              >
                <span className={n.on ? 'text-brand' : 'text-mute'}>{n.icon}</span>
                {n.label}
              </a>
            ))}
          </nav>
          <div className="mt-auto space-y-1.5 rounded-xl border border-line bg-card p-3 text-[12px]">
            <Check ok={st?.ffmpeg} label="Video recorder" pending="Setting up…" />
            <Check ok={st?.claude} label="Claude" pending="Not found" />
            <Check ok={st?.roblox} label="Roblox game open" pending="Not open" quiet />
            {st?.vault && (
              <button onClick={() => call('open', { what: 'vault' })} className="mt-1 flex items-center gap-1.5 text-mute hover:text-ink">
                <FolderOpen className="size-3.5" /> Research vault
              </button>
            )}
          </div>
        </aside>
        <main className="min-w-0 flex-1 overflow-y-auto">
          <div className="mx-auto max-w-[1040px] px-8 py-7">
            {sid ? <SessionPage key={sid} id={sid} status={st} /> : studied ? <StudiedGame key={studied} id={studied} /> : route === '#/studied' ? <StudiedPage /> : route === '#/sessions' ? <SessionsPage /> : route === '#/playbook' ? <PlaybookPage /> : <HomePage />}
          </div>
        </main>
      </div>
    </div>
  )
}

function Check({ ok, label, pending, quiet }: { ok?: boolean; label: string; pending: string; quiet?: boolean }) {
  return (
    <div className="flex items-center gap-2">
      <span className={cx('size-1.5 rounded-full', ok ? 'bg-brand' : quiet ? 'bg-mute/60' : 'bg-amber')} />
      <span className="flex-1 text-ink-2">{label}</span>
      {!ok && <span className="text-mute">{pending}</span>}
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
