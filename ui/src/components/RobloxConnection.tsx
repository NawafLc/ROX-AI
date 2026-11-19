import { CheckCircle2, ChevronDown, CircleAlert, ExternalLink, FlaskConical, KeyRound, Loader2, LockKeyhole, Unplug, XCircle } from 'lucide-react'
import { ReactNode, useState } from 'react'
import { call, useApi } from '../lib/api'
import { Badge, Button, Card, cx } from './ui'

type Step = { name: string; ok: boolean; detail: string; seconds: number }
type RStatus = {
  connection: { universe_id: number; place_id: number; place_name: string; connected_at: string } | null
  tools: { name: string; version: string; ok: boolean }[]
  tools_ready: boolean
  running: boolean
  now: { step: string } | null
  last: { ok: boolean; steps: Step[]; finished: string; place_version: number | null } | null
}

const DASHBOARD = 'https://create.roblox.com/dashboard/creations'
const KEYS = 'https://create.roblox.com/dashboard/credentials?activeTab=ApiKeysTab'
const open = (url: string) => call('open', { what: 'url', url })

/** Settings → Game building: the private test place, the API key and the build pipeline. */
export default function RobloxConnection() {
  const { data, reload } = useApi<RStatus>('roblox_status', {}, 2000)
  if (!data)
    return (
      <Card className="flex items-center gap-2 p-4 text-[13px] text-mute">
        <Loader2 className="size-4 animate-spin" /> Loading…
      </Card>
    )
  return (
    <Card className="divide-y divide-line">
      {data.connection ? <Connected s={data} reload={reload} /> : <Connect onDone={reload} />}
      <Pipeline s={data} reload={reload} />
    </Card>
  )
}

function Connected({ s, reload }: { s: RStatus; reload: () => void }) {
  const [confirm, setConfirm] = useState(false)
  const c = s.connection!
  return (
    <div className="flex items-center gap-4 px-4 py-3.5">
      <div className="grid size-9 place-items-center rounded-lg bg-brand/12 text-brand">
        <CheckCircle2 className="size-5" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-medium">
          Connected to <span className="text-ink">{c.place_name}</span>
        </div>
        <div className="text-[12.5px] text-mute">
          Private test place {c.place_id} · your key is saved on this PC only, encrypted
        </div>
      </div>
      {confirm ? (
        <div className="flex gap-2">
          <Button size="sm" kind="ghost" onClick={() => setConfirm(false)}>
            Cancel
          </Button>
          <Button size="sm" kind="danger" onClick={() => call('roblox_disconnect').then(reload)}>
            Disconnect
          </Button>
        </div>
      ) : (
        <Button size="sm" kind="ghost" icon={<Unplug className="size-3.5" />} onClick={() => setConfirm(true)}>
          Disconnect
        </Button>
      )}
    </div>
  )
}

function Connect({ onDone }: { onDone: () => void }) {
  const [place, setPlace] = useState('')
  const [key, setKey] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const go = () => {
    setBusy(true)
    setErr('')
    call('roblox_connect', { key, place })
      .then(() => {
        setKey('')
        onDone()
      })
      .catch((e) => setErr(String(e?.message ?? e)))
      .finally(() => setBusy(false))
  }
  return (
    <div className="p-4">
      <div className="flex items-start gap-3">
        <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-amber/12 text-amber">
          <KeyRound className="size-5" />
        </div>
        <div>
          <div className="flex items-center gap-2 text-[13.5px] font-medium">
            Connect your Roblox test place <Badge tone="amber">One time</Badge>
          </div>
          <div className="text-[12.5px] text-mute">Your team uploads every build here and tests it on Roblox's servers. It stays private, and it's free.</div>
        </div>
      </div>

      <div className="mt-4 space-y-2">
        <Guide n={1} title="Make a private test place" link={['Open Creator Dashboard', DASHBOARD]}>
          In <b>Roblox Studio</b>, signed in with your <b>alt account</b>: <b>File → New → Baseplate</b>, then <b>File → Publish to Roblox</b> → new experience named <b>ROX-AI Test</b>. New
          experiences are private. Then in Creator Dashboard click <b>⋯</b> on it → <b>Copy Start Place ID</b>.
        </Guide>
        <Guide n={2} title="Create a free API key" link={['Open API keys', KEYS]}>
          <b>Create API Key</b>, name it <b>ROX-AI</b>. Under Access permissions add <b>universe-places</b> → ROX-AI Test → <b>Write</b>, and <b>luau-execution-sessions</b> → ROX-AI
          Test → <b>Read</b> and <b>Write</b>. Accepted IP addresses: <b>0.0.0.0/0</b>. No expiration. <b>Save &amp; Generate key</b>, then <b>Copy</b>.
        </Guide>
      </div>

      <div className="mt-4 grid grid-cols-[1fr_1.4fr_auto] items-end gap-2.5">
        <Field label="Test place" value={place} onChange={setPlace} placeholder="Start Place ID or link" />
        <Field label="API key" value={key} onChange={setKey} placeholder="Paste the key" secret />
        <Button kind="primary" loading={busy} disabled={!place.trim() || !key.trim()} onClick={go}>
          {busy ? 'Checking with Roblox…' : 'Connect'}
        </Button>
      </div>
      {err && (
        <div className="fade-in mt-3 flex gap-2 rounded-lg border border-rose/30 bg-rose/8 px-3 py-2.5 text-[12.5px] text-ink-2">
          <CircleAlert className="mt-px size-4 shrink-0 text-rose" />
          <span className="selectable">{err}</span>
        </div>
      )}
      <div className="mt-3 flex items-center gap-1.5 text-[12px] text-mute">
        <LockKeyhole className="size-3.5" /> The key is saved only on this PC, encrypted for your Windows account. Your AI team can never see it.
      </div>
    </div>
  )
}

function Guide({ n, title, link, children }: { n: number; title: string; link: [string, string]; children: ReactNode }) {
  const [openStep, setOpen] = useState(n === 1)
  return (
    <div className="rounded-lg border border-line bg-bg/60">
      <button onClick={() => setOpen(!openStep)} className="flex w-full items-center gap-3 px-3 py-2.5 text-left">
        <span className="grid size-5 place-items-center rounded-full bg-card-2 text-[11px] font-bold text-ink-2">{n}</span>
        <span className="flex-1 text-[13px] font-medium">{title}</span>
        <ChevronDown className={cx('size-4 text-mute transition-transform', openStep && 'rotate-180')} />
      </button>
      {openStep && (
        <div className="fade-in px-3 pb-3 pl-11">
          <p className="text-[12.5px] leading-relaxed text-ink-2 [&_b]:font-semibold [&_b]:text-ink">{children}</p>
          <Button size="sm" className="mt-2.5" icon={<ExternalLink className="size-3.5" />} onClick={() => open(link[1])}>
            {link[0]}
          </Button>
        </div>
      )}
    </div>
  )
}

function Field({ label, value, onChange, placeholder, secret }: { label: string; value: string; onChange: (v: string) => void; placeholder: string; secret?: boolean }) {
  return (
    <label className="block min-w-0">
      <div className="mb-1 text-[12px] font-medium text-mute">{label}</div>
      <input
        type={secret ? 'password' : 'text'}
        autoComplete="off"
        spellCheck={false}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="selectable h-10 w-full rounded-[10px] border border-line bg-bg px-3 text-[13px] text-ink outline-none placeholder:text-mute focus:border-brand/60"
      />
    </label>
  )
}

function Pipeline({ s, reload }: { s: RStatus; reload: () => void }) {
  const last = s.last
  const start = () => call('pipeline_test').then(() => setTimeout(reload, 400))
  return (
    <div className="px-4 py-3.5">
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-[13.5px] font-medium">
            Build pipeline
            {s.tools.map((t) => (
              <Badge key={t.name} tone={t.ok ? 'brand' : 'neutral'}>
                {t.name === 'rojo' ? 'Rojo' : 'Lune'} {t.version}
              </Badge>
            ))}
          </div>
          <div className="text-[12.5px] text-mute">
            {s.running
              ? `${s.now?.step ?? 'Working'}…`
              : s.connection
                ? 'Builds the starter game, checks every script, uploads it and runs the tests on Roblox.'
                : 'Builds the starter game and checks every script on this PC. Connect Roblox to also test on its servers.'}
          </div>
        </div>
        <Button size="sm" loading={s.running} icon={<FlaskConical className="size-3.5" />} onClick={start}>
          {s.running ? 'Testing' : 'Test pipeline'}
        </Button>
      </div>
      {last && !s.running && (
        <div className="fade-in mt-3 space-y-1.5 rounded-lg border border-line bg-bg/60 p-3">
          {last.steps.map((st) => (
            <div key={st.name} className="flex gap-2.5 text-[12.5px]">
              {st.ok ? <CheckCircle2 className="mt-px size-4 shrink-0 text-brand" /> : <XCircle className="mt-px size-4 shrink-0 text-rose" />}
              <div className="min-w-0 flex-1">
                <div className="flex justify-between gap-3">
                  <span className="font-medium text-ink">{st.name}</span>
                  <span className="tabular-nums text-mute">{st.seconds.toFixed(1)}s</span>
                </div>
                <div className={cx('selectable whitespace-pre-wrap break-words', st.ok ? 'text-mute' : 'text-ink-2')}>{st.detail}</div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
