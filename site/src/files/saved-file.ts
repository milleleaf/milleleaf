// Saving a file back to where it was opened from, through the browser's File System Access API
// (Chromium only; every caller feature-detects with canPickFiles first). No framework imports on
// purpose: this module is written so it can move into a reusable custom element later.

export type SaveState = 'unsaved' | 'saving' | 'saved' | 'failed' | 'conflict'

// The permission methods are Chrome-specific and not in the standard DOM typings yet. A handle
// from a picker can already write, so the absence of both methods means "granted", not "denied".
export type SourceHandle = FileSystemFileHandle & {
  queryPermission?(descriptor: { mode: 'read' | 'readwrite' }): Promise<PermissionState>
  requestPermission?(descriptor: { mode: 'read' | 'readwrite' }): Promise<PermissionState>
}

// One opened file: the handle it was opened from, plus the file's size and modification time as
// the page last saw them. That baseline is what the conflict check compares against.
export type Source = { handle: SourceHandle; baseline: { lastModified: number; size: number } }

export type SaveResult = {
  state: 'saved' | 'conflict' | 'failed' | 'cancelled'
  error?: string
  source: Source
}

// Saving to a new file has no conflict check, so it can only succeed, fail or be cancelled.
export type SaveAsResult = { state: 'saved' | 'failed' | 'cancelled'; error?: string; source?: Source }

declare global {
  interface Window {
    showOpenFilePicker?(options?: { types?: { description?: string; accept: Record<string, string[]> }[]; multiple?: boolean }): Promise<FileSystemFileHandle[]>
    showSaveFilePicker?(options?: { suggestedName?: string; types?: { description?: string; accept: Record<string, string[]> }[] }): Promise<FileSystemFileHandle>
  }
}

export function canPickFiles(): boolean {
  return typeof window.showOpenFilePicker === 'function' && typeof window.showSaveFilePicker === 'function'
}

const message = (error: unknown): string => (error as Error)?.message || String(error)

export const conflictMessage = (name: string) => `${name} changed on disk since you opened it.`

export async function openFile(accept: Record<string, string[]>): Promise<{ file: File; source: Source } | null> {
  let handles: FileSystemFileHandle[]
  try {
    // The picker must be the very first call, before any await: only a fresh user activation
    // opens it at all.
    handles = await window.showOpenFilePicker!({ types: [{ accept }] })
  } catch (error) {
    if ((error as DOMException).name === 'AbortError') return null // the visitor closed the picker
    throw error
  }
  const handle: SourceHandle = handles[0]
  const file = await handle.getFile()
  return { file, source: { handle, baseline: { lastModified: file.lastModified, size: file.size } } }
}

// Writing needs readwrite permission; requestPermission must run inside the click's activation.
async function writePermission(handle: SourceHandle): Promise<string | undefined> {
  let state = handle.queryPermission ? await handle.queryPermission({ mode: 'readwrite' }) : 'granted'
  if (state === 'granted') return undefined
  state = handle.requestPermission ? await handle.requestPermission({ mode: 'readwrite' }) : 'granted'
  return state === 'granted' ? undefined : 'The browser did not allow writing to this file.'
}

// The browser stages the written bytes until close(); a save is confirmed only when close()
// resolves. Any failure aborts the staged write, so no half-written file is left behind.
async function writeBlob(handle: SourceHandle, blob: BlobPart): Promise<string | undefined> {
  let writable: FileSystemWritableFileStream | undefined
  try {
    writable = await handle.createWritable()
    await writable.write(blob)
    await writable.close()
    return undefined
  } catch (error) {
    if (writable) { try { await writable.abort() } catch { /* aborting a broken stream is best effort */ } }
    return `The write failed: ${message(error)}`
  }
}

// A read error must never pass for "unchanged": the caller reports a failure instead of
// overwriting a file it could not look at.
async function freshBaseline(handle: SourceHandle): Promise<{ baseline?: Source['baseline']; error?: string }> {
  try {
    const file = await handle.getFile()
    return { baseline: { lastModified: file.lastModified, size: file.size } }
  } catch {
    return { error: "couldn't check the file on disk" }
  }
}

export async function saveToSource(source: Source, blob: BlobPart): Promise<SaveResult> {
  const handle = source.handle
  const refused = await writePermission(handle)
  if (refused) return { state: 'failed', error: refused, source }
  const current = await freshBaseline(handle)
  if (current.error) return { state: 'failed', error: current.error, source }
  // File size and modification time catch most outside edits, but they are a check, not a lock:
  // a writer that keeps both stable can still slip past. On any difference, write nothing.
  if (current.baseline!.lastModified !== source.baseline.lastModified || current.baseline!.size !== source.baseline.size) {
    return { state: 'conflict', error: conflictMessage(handle.name), source }
  }
  const failed = await writeBlob(handle, blob)
  if (failed) return { state: 'failed', error: failed, source }
  // Confirm with a fresh read after close(); it becomes the baseline the next save compares
  // against, and it proves the file now holds what was written.
  const fresh = await freshBaseline(handle)
  if (fresh.error) return { state: 'failed', error: fresh.error, source }
  return { state: 'saved', source: { handle, baseline: fresh.baseline! } }
}

// "Overwrite anyway": the same save without the conflict check, after the visitor has chosen to
// replace a file that changed on disk under them.
export async function overwriteSource(source: Source, blob: BlobPart): Promise<SaveResult> {
  const handle = source.handle
  const refused = await writePermission(handle)
  if (refused) return { state: 'failed', error: refused, source }
  const failed = await writeBlob(handle, blob)
  if (failed) return { state: 'failed', error: failed, source }
  const fresh = await freshBaseline(handle)
  if (fresh.error) return { state: 'failed', error: fresh.error, source }
  return { state: 'saved', source: { handle, baseline: fresh.baseline! } }
}

// "Save as…": the picker must be the very first call, before any await, so the click's activation
// opens it; the chosen handle becomes the new source.
export async function saveAsNew(suggestedName: string, accept: Record<string, string[]>, blob: BlobPart): Promise<SaveAsResult> {
  let handle: FileSystemFileHandle
  try {
    handle = await window.showSaveFilePicker!({ suggestedName, types: [{ accept }] })
  } catch (error) {
    if ((error as DOMException).name === 'AbortError') return { state: 'cancelled' }
    return { state: 'failed', error: message(error) }
  }
  const failed = await writeBlob(handle, blob)
  if (failed) return { state: 'failed', error: failed }
  const fresh = await freshBaseline(handle)
  if (fresh.error) return { state: 'failed', error: fresh.error }
  return { state: 'saved', source: { handle, baseline: fresh.baseline! } }
}