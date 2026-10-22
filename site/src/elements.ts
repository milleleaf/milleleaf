// Reusable leaves own their shadow trees; React owns the page.
const shared = `:host{font:inherit;color:inherit}*{box-sizing:border-box}button{font:inherit;cursor:pointer;color:var(--ink);background:var(--paper);border:1px solid var(--line);border-radius:0;padding:10px 13px}button:focus-visible{outline:3px solid var(--focus);outline-offset:4px}`

export function registerElements() {
  if (customElements.get('ml-theme')) return
  class ThemeSwitch extends HTMLElement {
    connectedCallback() {
      if (this.shadowRoot) return
      const root = this.attachShadow({ mode: 'open' })
      root.innerHTML = `<style>${shared}button{min-width:78px;font-size:13px}</style><button type="button" aria-label="Switch colour theme"></button>`
      const button = root.querySelector('button')!
      const current = () => document.documentElement.dataset.theme || (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
      const update = () => { button.textContent = current() === 'dark' ? 'Light mode' : 'Dark mode' }
      update()
      button.addEventListener('click', () => {
        const theme = current() === 'dark' ? 'light' : 'dark'
        document.documentElement.dataset.theme = theme
        try { localStorage.setItem('milleleaf-theme', theme) } catch { /* Storage is optional. */ }
        update()
        this.dispatchEvent(new CustomEvent('theme-change', { detail: { theme }, bubbles: true, composed: true }))
      })
    }
  }
  class DemoFrame extends HTMLElement {
    private config?: { src: string; title: string }
    set options(value: { src: string; title: string }) { this.config = value; void this.load() }
    connectedCallback() { if (!this.shadowRoot) this.attachShadow({ mode: 'open' }); void this.load() }
    private async load() {
      if (!this.isConnected || !this.config || this.shadowRoot?.childElementCount) return
      const { src, title } = this.config
      try {
        const response = await fetch(src, { signal: AbortSignal.timeout(5000) })
        if (!response.ok || !response.headers.get('content-type')?.includes('text/html')) return
        const html = await response.text()
        // Static hosts can answer a missing path with the website's own HTML.
        if (html.includes('name="milleleaf-site"') || !/<(?:html|body|script)[\s>]/i.test(html)) return
        const root = this.shadowRoot!
        root.innerHTML = `<style>:host{display:block}iframe{width:100%;height:640px;border:1px solid var(--line);border-radius:12px;background:var(--paper)}</style>`
        const frame = document.createElement('iframe')
        frame.src = src
        frame.title = title
        root.append(frame)
        this.dispatchEvent(new CustomEvent('demo-ready', { bubbles: true, composed: true }))
      } catch { /* Keep the server-rendered availability message visible. */ }
    }
  }
  class RevisionSelector extends HTMLElement {
    private config = { labels: [] as string[], selected: 1 }
    set options(value: { labels: string[]; selected: number }) { this.config = value; this.render() }
    connectedCallback() { if (!this.shadowRoot) this.attachShadow({ mode: 'open' }); this.render() }
    private render() {
      if (!this.shadowRoot || !this.config.labels.length) return
      this.shadowRoot.innerHTML = `<style>${shared}:host{display:block}nav{display:flex;gap:6px;flex-wrap:wrap}button{font-size:13px;color:var(--tool-ink);background:transparent;border-color:var(--tool-ink);padding:10px 12px}button[aria-pressed=true]{background:var(--tool-ink);color:var(--mint)}@media(max-width:800px){nav{display:grid;grid-template-columns:1fr 1fr}button{font-size:11px;padding:9px 5px;text-align:left}}</style><nav aria-label="Fixture revisions"></nav>`
      this.config.labels.forEach((label, i) => {
        const button = document.createElement('button')
        button.type = 'button'
        button.textContent = `${i + 1}. ${label}`
        button.setAttribute('aria-pressed', String(i === this.config.selected))
        button.addEventListener('click', () => {
          this.config.selected = i
          this.shadowRoot!.querySelectorAll('button').forEach((b, n) => b.setAttribute('aria-pressed', String(n === i)))
          this.dispatchEvent(new CustomEvent('revision-select', { detail: { index: i }, bubbles: true, composed: true }))
        })
        button.addEventListener('keydown', event => {
          if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') return
          event.preventDefault()
          const next = (i + (event.key === 'ArrowRight' ? 1 : -1) + this.config.labels.length) % this.config.labels.length
          const target = this.shadowRoot!.querySelectorAll('button')[next]
          target.focus(); target.click()
        })
        this.shadowRoot!.querySelector('nav')!.append(button)
      })
    }
  }
  class VerifyLayer extends HTMLElement {
    private config?: { bytes: string; layer: string; key: string; signature: string }
    set options(value: { bytes: string; layer: string; key: string; signature: string }) { this.config = value }
    connectedCallback() {
      if (this.shadowRoot) return
      const view = this.attachShadow({ mode: 'open' })
      view.innerHTML = `<style>${shared}:host{display:block;max-width:220px}button{color:var(--mint);border-color:var(--mint);background:transparent;font-size:12px;padding:10px}p{font-size:11px;line-height:1.5;margin:8px 0;color:var(--mint)}</style><button type="button">Check in this browser</button><p role="status">SHA-256 + Ed25519. Full hybrid check ran with ox.</p>`
      view.querySelector('button')!.addEventListener('click', async () => {
        const button = view.querySelector('button')!
        const status = view.querySelector('p')!
        button.disabled = true
        try {
          if (!this.config) throw new Error('Evidence is still loading. Try again.')
          const data = this.config
          const hash = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(data.bytes)))).map(n => n.toString(16).padStart(2, '0')).join('')
          if ('sha256-' + hash !== data.layer) throw new Error('Layer hash does not match.')
          const decode = (text: string) => Uint8Array.from(atob(text), c => c.charCodeAt(0)).buffer as ArrayBuffer
          const key = await crypto.subtle.importKey('raw', decode(data.key.split(':')[1]), 'Ed25519', false, ['verify'])
          const valid = await crypto.subtle.verify('Ed25519', key, decode(data.signature), new TextEncoder().encode('Milleleaf layer signature v1\n' + data.layer))
          if (!valid) throw new Error('Signature does not match.')
          status.textContent = '✓ Layer hash and Ed25519 signature match in this browser.'
          this.dispatchEvent(new CustomEvent('verification-result', { detail: { valid: true }, bubbles: true, composed: true }))
        } catch (error) {
          status.textContent = error instanceof Error ? error.message : 'This browser could not run the check.'
        } finally { button.disabled = false }
      })
    }
  }
  customElements.define('ml-theme', ThemeSwitch)
  customElements.define('ml-demo', DemoFrame)
  customElements.define('ml-revisions', RevisionSelector)
  customElements.define('ml-verify', VerifyLayer)
}
