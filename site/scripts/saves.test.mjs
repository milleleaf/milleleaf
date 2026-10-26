import test from 'node:test'
import assert from 'node:assert/strict'
import { savedEntry } from '../src/office/library.ts'

const original = { id: 'original-workbook', name: 'finance.xlsx', tool: 'gridcraft', bytes: new Uint8Array([80, 75, 3, 4]).buffer, modified: 1 }
const request = (name, saveAs) => ({ name, bytes: new TextEncoder().encode(name).buffer, saveAs })

test('ordinary workbook Save updates the current entry', () => {
  const { file, updatesCurrent } = savedEntry('gridcraft', request(original.name), original)
  assert.equal(file.id, original.id)
  assert.equal(file.tool, 'gridcraft')
  assert.equal(updatesCurrent, true)
})

test('GridCraft Save As without a flag preserves the original workbook', () => {
  const { file, updatesCurrent } = savedEntry('gridcraft', request('finance-copy.xlsx'), original)
  assert.notEqual(file.id, original.id)
  assert.equal(file.name, 'finance-copy.xlsx')
  assert.equal(updatesCurrent, true)
})

test('PDF export gets a separate PDF entry and does not retarget subsequent workbook saves', () => {
  const desk = new Map([[original.id, original]])
  const exported = savedEntry('gridcraft', request('finance.PDF'), original)
  desk.set(exported.file.id, exported.file)
  assert.equal(desk.size, 2)
  assert.equal(desk.get(original.id), original)
  assert.equal(exported.file.tool, 'pdfcraft')
  assert.equal(exported.updatesCurrent, false)
  const subsequent = savedEntry('gridcraft', request(original.name), original)
  desk.set(subsequent.file.id, subsequent.file)
  assert.equal(subsequent.file.id, original.id)
  assert.equal(desk.get(exported.file.id).tool, 'pdfcraft')
  assert.equal(desk.size, 2)
})

test('a new GridCraft document with no desk association gets a new entry', () => {
  const { file } = savedEntry('gridcraft', request('Book1.xlsx'))
  assert.ok(file.id)
  assert.notEqual(file.id, original.id)
})

test('PdfCraft explicit Save As preserves even an identically named source', () => {
  const source = { ...original, tool: 'pdfcraft', name: 'finance.pdf' }
  const ordinary = savedEntry('pdfcraft', request(source.name), source)
  assert.equal(ordinary.file.id, source.id)
  const copy = savedEntry('pdfcraft', request(source.name, true), source)
  assert.notEqual(copy.file.id, source.id)
  assert.equal(copy.updatesCurrent, true)
})
