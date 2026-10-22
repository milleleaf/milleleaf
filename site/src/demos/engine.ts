import { useEffect, useRef, useState } from 'react'

// The document engine (Rust compiled to WebAssembly) runs in a module worker, so pages stay responsive.
// Both demos talk to it the same way: numbered requests, one reply each.
export type Call = (method: string, args?: unknown[], transfer?: Transferable[]) => Promise<any>
export type EngineState = { status: 'loading' | 'ready' | 'failed'; error?: string }

export function useEngine(url: string): [Call, EngineState] {
  const call = useRef<Call>(() => Promise.reject(new Error('The local document engine is still loading.')))
  const [state, setState] = useState<EngineState>({ status: 'loading' })
  useEffect(() => {
    const worker = new Worker(url, { type: 'module' })
    const pending = new Map<number, { resolve: (v: any) => void; reject: (e: Error) => void }>()
    let sequence = 0
    worker.onmessage = ({ data }) => {
      const request = pending.get(data.id)
      if (!request) return
      pending.delete(data.id)
      if (data.error) request.reject(new Error(data.error))
      else request.resolve(data.result)
    }
    worker.onerror = event => {
      const error = new Error(event.message || 'The local document engine could not load.')
      for (const request of pending.values()) request.reject(error)
      pending.clear()
      setState({ status: 'failed', error: error.message })
    }
    call.current = (method, args = [], transfer = []) => new Promise((resolve, reject) => {
      const id = ++sequence
      pending.set(id, { resolve, reject })
      worker.postMessage({ id, method, args }, transfer)
    })
    call.current('ready').then(() => setState({ status: 'ready' }), (error: Error) => setState({ status: 'failed', error: error.message }))
    return () => worker.terminate()
  }, [url])
  return [(...a) => call.current(...a), state]
}

export const json = (value: unknown) => JSON.stringify(value, null, 2)

// Save bytes the engine made as a file, without any network request.
export function saveFile(bytes: BlobPart, name: string, type = 'application/octet-stream') {
  const url = URL.createObjectURL(new Blob([bytes], { type }))
  const link = document.createElement('a')
  link.href = url
  link.download = name
  link.click()
  setTimeout(() => URL.revokeObjectURL(url), 30_000)
}
