import { Minus, Square, X } from 'lucide-react'
import { isTauri } from '../lib/api'
import { Logo } from './Logo'

async function win() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  return getCurrentWindow()
}

/** Our own dark title bar (the window has no Windows frame). Drag anywhere on it; double-click to maximize. */
export function TitleBar() {
  const btn = 'grid h-full w-[46px] place-items-center text-ink-2 transition-colors hover:bg-card-2 hover:text-ink'
  return (
    <div data-tauri-drag-region className="flex h-9 shrink-0 select-none items-stretch">
      <div data-tauri-drag-region className="flex w-[228px] shrink-0 items-center gap-2 border-r border-line bg-panel px-4">
        <Logo size={18} />
        <span data-tauri-drag-region className="text-[12.5px] font-bold tracking-tight text-ink">
          ROX-AI
        </span>
      </div>
      <div data-tauri-drag-region className="flex-1" />
      {isTauri && (
        <div className="flex">
          <button className={btn} onClick={() => win().then((w) => w.minimize())} aria-label="Minimize">
            <Minus className="size-4" />
          </button>
          <button className={btn} onClick={() => win().then((w) => w.toggleMaximize())} aria-label="Maximize">
            <Square className="size-3" />
          </button>
          <button className="grid h-full w-[46px] place-items-center text-ink-2 transition-colors hover:bg-[#e81123] hover:text-white" onClick={() => win().then((w) => w.close())} aria-label="Close">
            <X className="size-4" />
          </button>
        </div>
      )}
    </div>
  )
}
