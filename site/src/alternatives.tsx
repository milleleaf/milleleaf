export const alternatives = [
  { name: 'Copy/paste', cells: [
    ['A static copy; no refresh', 'Paste behaviour'],
    ['No return link', 'Paste behaviour'],
    ['Copies can cross apps; formatting is converted', 'Paste behaviour'],
    ['No loss report or source history', 'Paste behaviour'],
  ] },
  { name: 'Paste Link', cells: [
    ['Refreshes; moved files can break links', 'Reported'],
    ['One way; Google updates overwrite copy edits', 'Reported'],
    ['Links within each suite', 'Reported'],
    ['Loss reporting not documented in our sources', 'Reported'],
  ] },
  { name: 'Import/export', cells: [
    ['A one-time conversion', 'Conversion behaviour'],
    ['No return link from conversion alone', 'Conversion behaviour'],
    ['Files cross apps; each save can rewrite them', 'Found in our tests'],
    ['Three LibreOffice charts saved without data', 'Found in our tests'],
  ] },
  { name: 'Plugins', cells: [
    ['Refresh inside Microsoft Office', 'Reported'],
    ['Reverse edits not documented in our sources', 'Reported'],
    ['Microsoft Office; wider support not documented', 'Reported'],
    ['Loss reporting not documented in our sources', 'Reported'],
  ] },
  { name: 'Milleleaf', cells: [
    ['Linked values follow on CLI export', 'Found in our tests'],
    ['Reviewable edits; linked-value conflicts', 'Found in our tests'],
    ['Calc edit to WordCraft report and DeckCraft deck', 'Found in our tests'],
    ['Inventoried losses listed on checked files', 'Found in our tests'],
  ] },
]

export function AlternativesTable() {
  const headings = ['Approach', 'Stays current', 'Edits come back', 'Works across apps', 'Loss evidence']
  return <div className="alternatives-wrap"><table className="alternatives-table">
    <caption>What each approach gives you, and the evidence behind it.</caption>
    <thead><tr>{headings.map(h => <th scope="col" key={h}>{h}</th>)}</tr></thead>
    <tbody>{alternatives.map(row => <tr key={row.name}><th scope="row">{row.name}</th>{row.cells.map(([claim, level], i) => <td data-label={headings[i + 1]} key={claim}><span>{claim}</span><small>{level}</small></td>)}</tr>)}</tbody>
  </table></div>
}
