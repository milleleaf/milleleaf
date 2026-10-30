export type Tool = 'wordcraft' | 'gridcraft' | 'deckcraft' | 'pdfcraft' | 'cadcraft' | 'designcraft' | 'effectcraft' | 'figcraft' | 'filmcraft' | 'lightcraft' | 'photocraft' | 'solidcraft' | 'soundcraft' | 'vectorcraft'

// Which editor edits a file type, for a file one app hands to another (an app that only references a
// file, such as a drawing placed in a model, keeps reading it itself). Only types an editor here opens.
const owners: Record<string, Tool> = { dwg: 'cadcraft', dxf: 'cadcraft', xlsx: 'gridcraft', xls: 'gridcraft', ods: 'gridcraft', csv: 'gridcraft', pdf: 'pdfcraft', docx: 'wordcraft',
  sc: 'solidcraft', step: 'solidcraft', stp: 'solidcraft', iges: 'solidcraft', igs: 'solidcraft', '3mf': 'solidcraft', stl: 'solidcraft' }
export function ownerOf(name: string): Tool | undefined {
  const dot = name.lastIndexOf('.')
  return dot > 0 ? owners[name.slice(dot + 1).toLowerCase()] : undefined
}

// Official releases framed here that speak the apps' host messages (?host=parent), so the page can hand
// them a file. CADCraft's shipped in its 0.13.0 release and PdfCraft's by 0.14.0; add an app when its
// release has one.
export const framedBridge: ReadonlySet<Tool> = new Set<Tool>(['cadcraft', 'pdfcraft'])

// What each editor costs while it runs, in MiB, measured one at a time in headless Chromium on Linux
// (process memory with the app open and idle, or what closing it gave back, whichever was larger;
// at least 60). Most of it is the compiled program, which grows with the app's code, not its document.
// Re-measure with an app's new release when its weight looks off.
export const appWeightMiB: Record<Tool, number> = {
  wordcraft: 60, gridcraft: 94, deckcraft: 105, pdfcraft: 225, cadcraft: 60, solidcraft: 340, vectorcraft: 60,
  designcraft: 60, figcraft: 60, photocraft: 60, lightcraft: 534, filmcraft: 60, effectcraft: 215, soundcraft: 82,
}

// How much the running editors may use together. A browser limits each tab (about 2 GB in Firefox, 4 GB
// for Chrome on desktop, far less on phones), and every editor here runs in this page's tab. About a
// quarter of the device's memory, between 500 MB and 2.5 GB; without that hint (only Chromium reports
// it), 600 MB on touch devices and 1.5 GB elsewhere.
export function memoryBudgetMiB(): number {
  const gib = (navigator as Navigator & { deviceMemory?: number }).deviceMemory
  if (typeof gib === 'number' && gib > 0) return Math.round(Math.min(2500, Math.max(500, gib * 1024 / 4)))
  return matchMedia('(pointer: coarse)').matches ? 600 : 1500
}

// Hands a saved file to the browser as a download.
export function download(file: { name: string; bytes: ArrayBuffer }) {
  const url = URL.createObjectURL(new Blob([file.bytes]))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = file.name
  anchor.click()
  setTimeout(() => URL.revokeObjectURL(url), 30_000)
}
