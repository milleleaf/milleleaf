import type { ReactNode } from 'react'

// The demos' status line: a moving bar while the engine loads or a file is processed.
export function Status({ busy, children }: { busy: boolean; children: ReactNode }) {
  return <p id="status" className={`demo-status${busy ? ' working' : ''}`} role="status" aria-live="polite" aria-busy={busy}>
    {busy && <span className="working-bar" aria-hidden="true" />}{children}
  </p>
}

export function fileSize(bytes: number) {
  if (bytes < 1024) return `${bytes} bytes`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}
