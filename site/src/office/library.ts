export type Tool = 'wordcraft' | 'gridcraft' | 'deckcraft' | 'pdfcraft' | 'cadcraft' | 'designcraft' | 'effectcraft' | 'figcraft' | 'filmcraft' | 'lightcraft' | 'photocraft' | 'soundcraft' | 'vectorcraft'
export type SavedFile = { id: string; name: string; tool: Tool; bytes: ArrayBuffer; modified: number }

export function savedEntry(tool: Tool, message: { name: string; bytes: ArrayBuffer; saveAs?: boolean }, current?: SavedFile) {
  const targetTool = tool === 'gridcraft' && /\.pdf$/i.test(message.name) ? 'pdfcraft' : tool
  // GridCraft supplies no Save As flag, so a changed name must never replace the open file.
  const replace = !message.saveAs && current?.name === message.name && current?.tool === targetTool
  const file: SavedFile = { id: replace ? current.id : crypto.randomUUID(), name: message.name, tool: targetTool, bytes: message.bytes, modified: Date.now() }
  return { file, updatesCurrent: targetTool === tool }
}

let database: Promise<IDBDatabase> | undefined
function db() {
  return database ??= new Promise((resolve, reject) => {
    const request = indexedDB.open('milleleaf-office', 1)
    request.onupgradeneeded = () => request.result.createObjectStore('files', { keyPath: 'id' })
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => { database = undefined; reject(request.error) }
  })
}

export async function files(): Promise<SavedFile[]> {
  const connection = await db()
  return new Promise((resolve, reject) => {
    const request = connection.transaction('files').objectStore('files').getAll()
    request.onsuccess = () => resolve((request.result as SavedFile[]).sort((a, b) => b.modified - a.modified))
    request.onerror = () => reject(request.error)
  })
}

export async function saveFile(file: SavedFile): Promise<void> {
  const connection = await db()
  return new Promise((resolve, reject) => {
    const transaction = connection.transaction('files', 'readwrite')
    transaction.objectStore('files').put(file)
    transaction.oncomplete = () => resolve()
    transaction.onerror = () => reject(transaction.error)
    transaction.onabort = () => reject(transaction.error)
  })
}

export function download(file: Pick<SavedFile, 'name' | 'bytes'>) {
  const url = URL.createObjectURL(new Blob([file.bytes]))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = file.name
  anchor.click()
  setTimeout(() => URL.revokeObjectURL(url), 30_000)
}
