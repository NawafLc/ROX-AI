// One API for the whole interface. Inside the desktop app it goes through Tauri; in a browser (previews and,
// later, the iPhone app) it goes through ROX-AI's local web server. Same commands either way.
import { invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useRef, useState } from 'react'

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export async function call<T = any>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (isTauri) return invoke<T>('api', { cmd, args })
  const r = await fetch(`/api/${cmd}`, { method: 'POST', body: JSON.stringify(args) })
  const j = await r.json()
  if (!r.ok) throw new Error(j.error ?? 'Request failed')
  return j as T
}

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

// ---------- shared types ----------
export type Activity = { kind: 'idle' | 'learning' | 'scanning' | 'attention'; title: string; detail: string }
export type Status = {
  version: string
  mode: 'passive' | 'active'
  auto_run: boolean
  setup_complete: boolean
  activity: Activity
  busy: boolean
  stats: { games: number; learned: number; on_chart: number; chart_day: string | null; last_scan: string | null }
  blocked: string | null
  job: { id: string; text: string; progress: number | null } | null
  queued: string[]
  notice: { message: string; ok: boolean } | null
  update: { state: 'checking' | 'current' | 'available' | 'installing' | 'failed'; latest?: string; current?: string; error?: string; checked_at?: string }
  platform: string
}
export type Dep = { id: string; name: string; why: string; ok: boolean; required: boolean; action: { kind: 'install' | 'signin'; label: string } | null }
export type GameRow = {
  uid: number
  rank: number | null
  title: string
  genre: string
  playing: number
  like_ratio: number
  momentum: number
  source: string
  source_label: string
  last_analysis: string
  fun: number | null
  one_liner: string
  note: string | null
}
export type Note = { path: string; title: string; props: Record<string, string>; body: string }

// ---------- formatting ----------
export function fmt(n: number | null | undefined): string {
  if (n === null || n === undefined) return '–'
  const a = Math.abs(n)
  if (a >= 1e9) return (n / 1e9).toFixed(1).replace(/\.0$/, '') + 'B'
  if (a >= 1e6) return (n / 1e6).toFixed(1).replace(/\.0$/, '') + 'M'
  if (a >= 1e3) return (n / 1e3).toFixed(1).replace(/\.0$/, '') + 'K'
  return String(Math.round(n))
}

export function ago(iso: string | null | undefined): string {
  if (!iso) return 'never'
  const t = new Date(iso.length <= 10 ? iso + 'T00:00:00' : iso).getTime()
  const s = (Date.now() - t) / 1000
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.floor(s / 60)} min ago`
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`
  const d = Math.floor(s / 86400)
  return d === 1 ? 'yesterday' : `${d} days ago`
}

export type AgentState = 'working' | 'idle' | 'paused' | 'soon'
export type Agent = { id: string; name: string; role: string; model: string; model_id?: string; default_model?: string; active?: boolean; state: AgentState; text: string; since?: string | null }
export type Home = {
  summary: string[]
  learned_today: number
  new_today: number
  charts_today: boolean
  my_games: number
  agents: Agent[]
  working: number
  next: { uid: number; title: string; rank: number; reason: string }[]
}
/** A game the AI team made (filled in by the building milestones). */
export type MyGame = {
  id: string
  title: string
  genre: string
  stage: 'idea' | 'building' | 'testing' | 'ready' | 'published' | 'failed'
  pitch?: string
  summary?: string
  progress: number
  stage_text: string
  thumb?: string | null
  updated?: string
}

export type Entry = { at: string; by: string; text: string; why?: string }
export type TeamTask = { id: string; title: string; owner: string }
export type Team = {
  enabled: boolean
  running: boolean
  paused_until: string | null
  claude_ready: boolean
  blocked: string | null
  roblox_connected: boolean
  agents: Agent[]
  current: {
    slug: string
    title: string
    pitch: string
    genre: string
    art_style: string
    size: string
    stage: string
    stage_text: string
    progress: number
    done: number
    total: number
    doing: TeamTask[]
    activity: Entry[]
  } | null
  ready: number
  games_to_build: number
  parallel: boolean
  max_game_size: string
}
export type ChatMsg = { from: 'owner' | 'lead'; text: string; at: string }
