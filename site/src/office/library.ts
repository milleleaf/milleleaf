export type Tool = 'wordcraft' | 'gridcraft' | 'deckcraft' | 'pdfcraft' | 'cadcraft' | 'designcraft' | 'effectcraft' | 'figcraft' | 'filmcraft' | 'lightcraft' | 'photocraft' | 'soundcraft' | 'vectorcraft'

// Hands a saved file to the browser as a download.
export function download(file: { name: string; bytes: ArrayBuffer }) {
  const url = URL.createObjectURL(new Blob([file.bytes]))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = file.name
  anchor.click()
  setTimeout(() => URL.revokeObjectURL(url), 30_000)
}
