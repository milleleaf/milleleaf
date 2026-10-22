import { datasets } from './datasets'

export function DatasetTables() {
  return <div className="dataset-catalogue">
    {(['open', 'other'] as const).map(group => <section key={group} aria-labelledby={`licences-${group}`}>
      <h2 id={`licences-${group}`}>
        {group === 'open' ? <>Open licences<span>: the data in <a href="https://worldviews-libre.pages.dev">WorldViews Libre</a></span></> : <>Other terms<span>: credited, not open: the extra data in <a href="https://worldviews.pages.dev">WorldViews</a></span></>}
      </h2>
      <div className="dataset-table-container">
        <table className="dataset-table" aria-labelledby={`licences-${group}`}>
          <thead><tr>{['Dataset', 'What it is', 'Area', 'Updated', 'Licence', 'Source', 'Links'].map(label => <th key={label} scope="col">{label}</th>)}</tr></thead>
          <tbody>{datasets.filter(d => d.group === group).map(d => <tr key={d.name}>
            <th scope="row" data-label="Dataset"><a href={`https://huggingface.co/datasets/milleleaf/${d.name}`}>{d.name}</a></th>
            <td data-label="What it is">{d.description}</td>
            <td data-label="Area">{d.area}</td>
            <td data-label="Updated">{d.updated}</td>
            <td data-label="Licence"><ul>{d.licences.map(l => <li key={l.href}><a href={l.href}>{l.label}</a></li>)}</ul></td>
            <td data-label="Source">{d.source}</td>
            <td data-label="Links"><ul>
              <li><a href={`https://huggingface.co/datasets/milleleaf/${d.name}`} aria-label={`${d.name} on Hugging Face`}>Hugging Face</a></li>
              <li><a href={`https://github.com/milleleaf/${d.pipeline || d.name}`} aria-label={`${d.name} GitHub pipeline`}>GitHub</a></li>
              <li><a href={`https://www.kaggle.com/datasets/helenkwok/${d.kaggle}`} aria-label={`${d.name} on Kaggle`}>{d.kaggleLabel || 'Kaggle'}</a></li>
            </ul></td>
          </tr>)}</tbody>
        </table>
      </div>
      {group === 'open' && <p className="dataset-note">The World Heritage view is not affiliated with or endorsed by UNESCO.</p>}
    </section>)}
  </div>
}
