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

// Hands a saved file to the browser as a download.
export function download(file: { name: string; bytes: ArrayBuffer }) {
  const url = URL.createObjectURL(new Blob([file.bytes]))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = file.name
  anchor.click()
  setTimeout(() => URL.revokeObjectURL(url), 30_000)
}
