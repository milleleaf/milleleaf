// Reusable leaves own their shadow trees; React owns the page.
const shared = `:host{font:inherit;color:inherit}*{box-sizing:border-box}button{font:inherit;cursor:pointer;color:var(--ink);background:var(--surface);border:1px solid var(--line);border-radius:6px;padding:10px 13px}button:focus-visible{outline:3px solid var(--focus);outline-offset:4px}`

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
        root.innerHTML = `<style>:host{display:block}iframe{width:100%;height:640px;border:1px solid var(--line);border-radius:12px;background:var(--surface)}</style>`
        const frame = document.createElement('iframe')
        frame.src = src
        frame.title = title
        root.append(frame)
        this.dispatchEvent(new CustomEvent('demo-ready', { bubbles: true, composed: true }))
      } catch { /* Keep the server-rendered availability message visible. */ }
    }
  }
  class LayerView extends HTMLElement {
    connectedCallback() {
      if (this.shadowRoot) return
      const root = this.attachShadow({ mode: 'open' })
      root.innerHTML = `<style>${shared}:host{display:block}nav{display:flex;flex-wrap:wrap;gap:6px;margin-bottom:22px}button{font-size:12px;background:transparent}button[aria-pressed=true]{background:var(--ink);color:var(--bg);border-color:var(--ink)}.sheet{padding:22px;background:var(--bg);border:1px solid var(--line);border-left:4px solid var(--teal);border-radius:5px}.label{font:11px monospace;letter-spacing:.09em;text-transform:uppercase;color:var(--muted)}h3{font-size:25px;margin:14px 0}p{line-height:1.7;margin:0;color:var(--muted)}.value{font:42px monospace;letter-spacing:-.07em;margin:18px 0}.meta{display:flex;justify-content:space-between;gap:12px;border-top:1px solid var(--line);padding-top:15px;font:12px monospace}</style><nav aria-label="Example presentations"><button type="button" aria-pressed="true">Workbook</button><button type="button" aria-pressed="false">Report</button><button type="button" aria-pressed="false">Deck</button></nav><div class="sheet" aria-live="polite"></div>`
      const names = ['Workbook', 'Report', 'Deck']
      const render = (i: number) => {
        root.querySelectorAll('button').forEach((b, n) => b.setAttribute('aria-pressed', String(n === i)))
        root.querySelector('.sheet')!.innerHTML = `<div class="label">${names[i]} · its own presentation</div><h3>${['A figure in a cell', 'A figure in your report', 'A figure on a slide'][i]}</h3><div class="value">42,800</div><p>One source value. Each place carries its own style.</p><div class="meta"><span>source: project figure</span><span>layer 01</span></div>`
      }
      root.querySelectorAll('button').forEach((b, i) => b.addEventListener('click', () => render(i)))
      render(0)
    }
  }
  customElements.define('ml-theme', ThemeSwitch)
  customElements.define('ml-demo', DemoFrame)
  customElements.define('ml-layers', LayerView)
}
