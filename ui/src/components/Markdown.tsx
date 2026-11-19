import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { useEffect, useState } from 'react'
import { call } from '../lib/api'

/** Obsidian-flavoured Markdown → the app's reader. Wikilinks, callouts, embeds and dataview blocks. */
export function Markdown({ body, onLink }: { body: string; onLink: (target: string) => void }) {
  const src = prepare(body)
  return (
    <div className="prose-rox">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkCallouts]}
        urlTransform={(u) => u}
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(e) => {
                e.preventDefault()
                if (!href) return
                if (href.startsWith('wiki:')) onLink(decodeURIComponent(href.slice(5)))
                else if (href.startsWith('https://')) call('open', { what: 'url', url: href })
              }}
            >
              {children}
            </a>
          ),
          img: ({ src, alt }) => <VaultImage src={String(src ?? '')} alt={alt ?? ''} />,
          code: ({ className, children }) => {
            if (className?.includes('language-dataview'))
              return <span className="text-mute">A live Obsidian table — open this note in Obsidian to see it.</span>
            return <code className={className}>{children}</code>
          },
        }}
      >
        {src}
      </ReactMarkdown>
    </div>
  )
}

function prepare(md: string): string {
  return (
    md
      // collapsible blocks just show their content
      .replace(/<\/?details>/g, '')
      .replace(/<summary>(.*?)<\/summary>/g, '**$1**\n')
      .replace(/<\/?sub>/g, '')
      // ![[image.png]] embeds
      .replace(/!\[\[([^\]|]+?)(?:\\?\|[^\]]*)?\]\]/g, (_, t) => `![](vimg:${encodeURI(t.trim())})`)
      // [[target|alias]] (also the table-escaped [[target\|alias]])
      .replace(/\[\[([^\]|\\]+?)(?:\\?\|([^\]]+))?\]\]/g, (_, t, alias) => `[${(alias ?? t.split('/').pop()).trim()}](wiki:${encodeURIComponent(t.trim())})`)
  )
}

// remark plugin: "> [!type] Title" blockquotes become styled callouts
type MdNode = { type: string; value?: string; children?: MdNode[]; data?: Record<string, unknown> }
function remarkCallouts() {
  return (tree: MdNode) => {
    const walk = (n: MdNode) => {
      if (n.type === 'blockquote') {
        const p = n.children?.[0]
        const t = p?.type === 'paragraph' ? p.children?.[0] : undefined
        const m = t?.type === 'text' ? /^\[!(\w+)\][+-]?[ \t]*([^\n]*)\n?/.exec(t.value ?? '') : null
        if (m && t && p) {
          const kind = m[1].toLowerCase()
          n.data = { hProperties: { className: ['callout', `callout-${kind}`] } }
          t.value = (t.value ?? '').slice(m[0].length)
          const title: MdNode = { type: 'strong', data: { hProperties: { className: ['callout-title'] } }, children: [{ type: 'text', value: m[2] || kind[0].toUpperCase() + kind.slice(1) }] }
          // inside callouts every line is its own line (as in Obsidian)
          const kids: MdNode[] = []
          for (const k of p.children ?? []) {
            if (k.type !== 'text' || !k.value?.includes('\n')) {
              kids.push(k)
              continue
            }
            k.value.split('\n').forEach((part, i) => {
              if (i) kids.push({ type: 'break' })
              if (part) kids.push({ type: 'text', value: part })
            })
          }
          p.children = [title, { type: 'break' }, ...kids]
        }
      }
      n.children?.forEach(walk)
    }
    walk(tree)
  }
}

const imgCache = new Map<string, string | null>()
function VaultImage({ src, alt }: { src: string; alt: string }) {
  const local = src.startsWith('vimg:') ? decodeURI(src.slice(5)) : !/^https?:/.test(src) ? decodeURI(src) : null
  const [data, setData] = useState<string | null | undefined>(local ? imgCache.get(local) : src)
  useEffect(() => {
    if (!local || imgCache.has(local)) return
    call<{ data: string | null }>('vault_image', { path: local }).then((r) => {
      imgCache.set(local, r.data)
      setData(r.data)
    })
  }, [local])
  if (!data) return null
  return <img src={data} alt={alt} />
}
