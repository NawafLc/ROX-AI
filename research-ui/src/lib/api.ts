// The engine lives in the app (research/src/core.rs). Every screen talks to it through `call`.
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useRef, useState } from 'react'

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export async function call<T = any>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!isTauri) {
    if (import.meta.env.DEV && import.meta.env.VITE_MOCK) return (await import('./mock')).mock(cmd, args)
    throw new Error('Open this inside the ROX Research app')
  }
  return invoke<T>('api', { cmd, args })
}

export const openOverlay = (id: string) => invoke('overlay_open', { id })
export const closeOverlay = (showMain: boolean, route = '') => invoke('overlay_close', { showMain, route })

/** A file on this PC as something an <img> or <video> can load. */
export const fileUrl = (path: string) => (isTauri ? convertFileSrc(path) : path)
export const join = (dir: string, file: string) => dir.replace(/[\\/]+$/, '') + '\\' + file.replace(/\//g, '\\')

/** Load a command's result, optionally re-polling every `ms`. */
export function useApi<T = any>(cmd: string, args: Record<string, unknown> = {}, ms?: number) {
  const [data, setData] = useState<T | undefined>(undefined)
  const [error, setError] = useState<string | undefined>(undefined)
  const key = JSON.stringify(args)
  const alive = useRef(true)
  const load = useCallback(async () => {
    try {
      const d = await call<T>(cmd, JSON.parse(key))
      if (alive.current) {
        setData(d)
        setError(undefined)
      }
    } catch (e) {
      if (alive.current) setError(String((e as Error).message ?? e))
    }
  }, [cmd, key])
  useEffect(() => {
    alive.current = true
    load()
    if (!ms) return () => void (alive.current = false)
    const t = setInterval(load, ms)
    return () => {
      alive.current = false
      clearInterval(t)
    }
  }, [load, ms])
  return { data, error, reload: load }
}

export type Game = {
  universe_id: number
  place_id: number
  name: string
  creator: string
  genre: string
  description: string
  playing: number
  visits: number
  icon: string
}
export type Shot = { title: string; goal: string; steps: string[]; minutes: number; look_for: string[] }
export type Segment = { index: number; file: string; seconds: number; notes: string; recorded_at: string; skipped: boolean }
export type Session = {
  id: string
  created: string
  game: Game
  status: 'planning' | 'ready' | 'recording' | 'recorded' | 'analyzing' | 'done' | 'failed'
  step: string
  error: string
  plan: Shot[]
  segments: Segment[]
  current: number
  final_notes: string
  teardown: string
}
export type SessionRow = { id: string; created: string; game: string; icon: string; status: Session['status']; step: string; segments: number; recorded: number; teardown: string }
export type Status = { ffmpeg: boolean; claude: boolean; roblox: boolean; recording: { id: string; index: number; seconds: number } | null; vault: string }

export const clock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`
export const compact = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)}B` : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(n ?? 0))

export function go(route: string) {
  window.location.hash = route
}
