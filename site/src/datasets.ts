type Link = { label: string; href: string }

export type Dataset = {
  name: string
  group: 'open' | 'other'
  description: string
  area: string
  updated: string
  licences: Link[]
  source: string
  pipeline?: string
  kaggle: string
  kaggleLabel?: string
}

const hkTerms = { label: 'DATA.GOV.HK terms', href: 'https://data.gov.hk/en/terms-and-conditions' }
const csdi = { label: 'CSDI terms', href: 'https://portal.csdi.gov.hk/csdi-webpage/doc/TNC' }
const odbl = { label: 'ODbL 1.0', href: 'https://opendatacommons.org/licenses/odbl/1-0/' }
const ccBy = { label: 'CC BY 4.0', href: 'https://creativecommons.org/licenses/by/4.0/' }
const cc0 = { label: 'CC0 1.0', href: 'https://creativecommons.org/publicdomain/zero/1.0/' }

// Card + repository descriptions checked 22 October 2026. These are data terms,
// not the pipeline code licences. A shared pipeline need not match the card name.
export const datasets: Dataset[] = [
  {
    name: 'hk-airport-flights', group: 'open',
    description: 'Hong Kong International Airport flight information, with each snapshot kept.',
    area: 'Hong Kong', updated: 'Every 15 minutes', licences: [hkTerms],
    source: 'Airport Authority Hong Kong',
    kaggle: 'hk-airport-flights-2026-10-21', kaggleLabel: 'Kaggle (21 Oct archive)',
  },
  {
    name: 'hk-buildings', group: 'open',
    description: 'Building footprints and heights from the Lands Department.',
    area: 'Hong Kong', updated: 'Weekly', licences: [csdi],
    source: 'Lands Department, Hong Kong SAR Government', kaggle: 'hk-buildings',
  },
  {
    name: 'hk-property', group: 'open',
    description: 'Property market statistics, original files and property area shapes.',
    area: 'Hong Kong', updated: 'Weekly', licences: [hkTerms, csdi],
    source: 'Rating and Valuation Department; Housing Authority; Land Registry', kaggle: 'hk-property',
  },
  {
    name: 'hk-roads', group: 'open',
    description: 'Road networks, traffic aids and road centrelines.',
    area: 'Hong Kong', updated: 'Weekly', licences: [hkTerms, csdi],
    source: 'Transport Department; Lands Department', kaggle: 'hk-roads',
  },
  {
    name: 'hk-transit', group: 'open',
    description: 'Bus, minibus, MTR and Light Rail routes and stops.',
    area: 'Hong Kong', updated: 'Daily', licences: [hkTerms, csdi],
    source: 'Transport Department; KMB/LWB; Citybus; Lands Department; MTR Corporation', kaggle: 'hk-transit',
  },
  {
    name: 'sa-planning', group: 'open',
    description: 'Planning zones, overlays, land use, height limits and building-rule hazard areas.',
    area: 'South Australia', updated: 'Weekly check; rebuilt on change',
    licences: [ccBy, { label: 'CC BY 3.0 AU in source ZIPs', href: 'https://huggingface.co/datasets/milleleaf/sa-planning#sources-and-terms' }],
    source: 'Department for Housing and Urban Development, Government of South Australia', kaggle: 'sa-planning',
  },
  {
    name: 'sa-transit', group: 'open',
    description: 'Public transport routes and stops, with live vehicle snapshots.',
    area: 'South Australia', updated: 'Weekly; live feeds every 15 minutes', licences: [ccBy],
    source: 'Department for Infrastructure and Transport (Adelaide Metro)', kaggle: 'sa-transit',
  },
  {
    name: 'world-airports', group: 'open',
    description: 'Airports, heliports and runways from OurAirports.',
    area: 'World', updated: 'Weekly',
    licences: [{ label: 'Public domain', href: 'https://ourairports.com/about.html' }],
    source: 'OurAirports contributors', kaggle: 'world-airports',
  },
  {
    name: 'world-buildings', group: 'open',
    description: 'Building footprints and heights as GeoParquet and 3D map tiles.',
    area: 'World', updated: 'Each Overture release; checked weekly',
    licences: [{ ...odbl, label: 'ODbL 1.0 (Overture)' }, { ...csdi, label: 'CSDI terms (Hong Kong)' }, { label: 'CC BY 4.0 (3DBAG)', href: 'https://docs.3dbag.nl/en/copyright/' }],
    source: 'Overture Maps Foundation / OpenStreetMap contributors; Lands Department; 3DBAG (tudelft3d and 3DGI)',
    kaggle: 'world-buildings-r001', kaggleLabel: 'Kaggle (region r001)',
  },
  {
    name: 'world-flights', group: 'open',
    description: 'Worldwide aircraft position traces and Hong Kong near-live snapshots.',
    area: 'World', updated: 'Daily; Hong Kong every 15 minutes', licences: [odbl],
    source: 'adsb.lol contributors', kaggle: 'world-flights',
  },
  {
    name: 'world-gazetteer', group: 'open',
    description: 'PlaceDB countries, administrative divisions and settlements with coordinates.',
    area: 'World', updated: 'Weekly check; new PlaceDB releases', licences: [cc0],
    source: 'PlaceDB (mindstellar/location-data), from Wikidata contributors', kaggle: 'world-gazetteer',
  },
  {
    name: 'world-heritage', group: 'open',
    description: 'World Heritage site points and mapped outlines, in separate files.',
    area: 'World', updated: 'Weekly',
    licences: [{ ...cc0, label: 'CC0 1.0 (points)' }, { ...odbl, label: 'ODbL 1.0 (outlines)' }],
    source: 'Wikidata contributors; OpenStreetMap contributors', kaggle: 'world-heritage',
  },
  {
    name: 'world-places', group: 'open',
    description: 'Official population figures for places absent from the World Bank API.',
    area: 'Selected places worldwide', updated: 'Weekly',
    licences: [{ label: 'Per-source open terms; see card', href: 'https://huggingface.co/datasets/milleleaf/world-places#sources-and-terms' }],
    source: 'Taiwan MOI; SPC; ÅSUB; ABS; INSEE; Statistics Jersey; Morocco HCP', kaggle: 'world-places',
  },
  {
    name: 'sa-fire', group: 'open',
    description: 'Fire history, last-fire boundaries, emergency markers and bushfire management areas.',
    area: 'South Australia', updated: 'Weekly check; rebuilt on change',
    licences: [{ label: "CC BY 4.0 (one source's 2015 zip notes CC BY 3.0 AU; see card)", href: 'https://huggingface.co/datasets/milleleaf/sa-fire' }],
    source: 'Department for Environment and Water; SA Country Fire Service', kaggle: 'sa-fire',
  },
  {
    name: 'world-building-green', group: 'other',
    description: 'Building energy codes and enforcement across 88 economies.',
    area: 'World', updated: 'Monthly',
    licences: [{ label: 'Noncommercial, with attribution', href: 'https://documents1.worldbank.org/curated/en/099718504112546491/pdf/IDU-1d303826-4a43-4eff-8649-1052aa316783.pdf' }],
    source: 'World Bank, Global Indicators Group', kaggle: 'world-building-green',
  },
  {
    name: 'world-places-nc', group: 'other',
    description: 'Official population figures republished with credit, without an open licence.',
    area: 'Selected places worldwide', updated: 'Weekly; Somaliland rechecked yearly',
    licences: [{ label: 'Per-source; no open licence', href: 'https://github.com/milleleaf/world-places/blob/main/sources.json' }],
    source: 'Guernsey Data & Analysis; TRNC Statistical Institute; Anguilla and Montserrat statistics departments; St Helena Statistics Office; Tristan da Cunha Association; Falklands Policy Department; Vatican Governatorato; Somaliland Central Statistics Department',
    pipeline: 'world-places', kaggle: 'world-places-nc',
  },
]
