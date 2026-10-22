"""Run on the box, from site/: python3 scripts/generate-hero.py.

Uses the actual ox CLI, the tracked LibreOffice report fixture and a fresh
LibreOffice re-save. Private test keys live only in a temporary directory.
"""
import datetime
import difflib
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]
REPORT = '3f1c2a90-0000-4c1e-9a00-000000000020'
WORKBOOK = '3f1c2a90-0000-4c1e-9a00-000000000010'
TABLE = '3f1c2a90-0000-4c1e-9a00-000000000012'
DECK = '2af44a75-c514-5c87-b208-689fa8b15950'
EXAMPLE = ROOT / 'examples/hello-table'
PROJECT = EXAMPLE / 'project.ox.json'
FIXTURE = EXAMPLE / 'fixtures/report.libreoffice-edited.docx'
NS = {'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'}
commands = []


def run(args, env=None):
    result = subprocess.run([str(a) for a in args], cwd=ROOT, env=env,
                            text=True, capture_output=True, check=True)
    commands.append(' '.join(str(a).replace(str(ROOT) + '/', '') for a in args))
    return result.stdout


def ox(*args):
    return run(['cargo', 'run', '-q', '-p', 'ox-cli', '--', *args], os.environ.copy())


def digest(data):
    return hashlib.sha256(data).hexdigest()


def document(path):
    with zipfile.ZipFile(path) as z:
        body = ET.fromstring(z.read('word/document.xml')).find('w:body', NS)
        blocks = []

        def walk(el):
            if el.tag == '{' + NS['w'] + '}p':
                text = ''.join(n.text or '' for n in el.findall('.//w:t', NS))
                if text.strip():
                    blocks.append({'kind': 'paragraph', 'text': text})
            elif el.tag == '{' + NS['w'] + '}tbl':
                rows = [[''.join(n.text or '' for n in cell.findall('.//w:t', NS))
                         for cell in row.findall('w:tc', NS)] for row in el.findall('w:tr', NS)]
                blocks.append({'kind': 'table', 'rows': rows})
            else:
                for child in el:
                    walk(child)
        walk(body)
        return blocks


def rewrites(before, after):
    with zipfile.ZipFile(before) as a, zipfile.ZipFile(after) as b:
        return [{'part': p, 'before': digest(a.read(p)), 'after': digest(b.read(p))}
                for p in ['word/document.xml', 'word/styles.xml']
                if p in a.namelist() and p in b.namelist() and a.read(p) != b.read(p)]


def segments(before, after):
    a, b = before.split(' '), after.split(' ')
    out = []
    for op, i, j, k, l in difflib.SequenceMatcher(None, a, b).get_opcodes():
        if op in ('delete', 'replace'):
            out.append({'kind': 'removed', 'text': ' '.join(a[i:j]) + ' '})
        if op in ('insert', 'replace'):
            out.append({'kind': 'added', 'text': ' '.join(b[k:l]) + ' '})
        if op == 'equal':
            out.append({'kind': 'same', 'text': ' '.join(b[k:l]) + ' '})
    return out


def flow_snapshot(project, work, label):
    """Check the actual exported tables and chart against the composed source."""
    source = json.loads(ox('get', project, TABLE))['attributes']['office::data::table']
    rows = [[str(row[0]), float(row[1])] for row in source['rows']]
    report = work / (label + '.docx')
    deck = work / (label + '.pptx')
    ox('docx-export', project, REPORT, '--out', report)
    ox('pptx-export', project, DECK, '--out', deck)
    report_blocks = document(report)
    report_rows = next(b['rows'] for b in report_blocks if b['kind'] == 'table')
    ns = {'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
          'c': 'http://schemas.openxmlformats.org/drawingml/2006/chart'}
    with zipfile.ZipFile(deck) as z:
        deck_tables = []
        for part in sorted(z.namelist()):
            if part.startswith('ppt/slides/slide') and part.endswith('.xml'):
                root = ET.fromstring(z.read(part))
                for table in root.findall('.//a:tbl', ns):
                    deck_tables.append([[''.join(n.text or '' for n in cell.findall('.//a:t', ns))
                                         for cell in row.findall('a:tc', ns)]
                                        for row in table.findall('a:tr', ns)])
        chart_part = next(p for p in z.namelist() if p.startswith('ppt/charts/chart') and p.endswith('.xml'))
        chart = ET.fromstring(z.read(chart_part))
        categories = [v.text for v in chart.findall('.//c:cat//c:pt/c:v', ns)]
        values = [float(v.text) for v in chart.findall('.//c:val//c:pt/c:v', ns)]
    def numeric(value):
        return float(value.replace('$', '').replace(',', '').replace('AUD', '').strip())
    def matches(table):
        return len(table) == len(rows) + 1 and all(
            actual[0] == expected[0] and numeric(actual[1]) == expected[1]
            for actual, expected in zip(table[1:], rows))
    assert matches(report_rows), 'Word export must contain the source table'
    deck_rows = next(table for table in deck_tables if matches(table))
    assert categories == [r[0] for r in rows], 'Deck chart categories must follow the workbook'
    assert values == [r[1] for r in rows], 'Deck chart values must follow the workbook'
    return {'source': source, 'report': {'blocks': report_blocks, 'sha256': digest(report.read_bytes())},
            'deck': {'rows': deck_rows, 'chart': {'categories': categories, 'values': values},
                     'sha256': digest(deck.read_bytes())},
            'checks': ['Word table matches workbook', 'Deck table matches workbook',
                       'Deck chart categories and values match workbook']}


with tempfile.TemporaryDirectory(prefix='milleleaf-site-hero-') as folder:
    work = Path(folder)
    os.environ['OX_KEYS'] = str(work / 'keys')
    original = work / 'original.docx'
    ox('docx-export', PROJECT, REPORT, '--out', original)
    ox('docx-import', PROJECT, REPORT, FIXTURE, '--out', work / 'edited.json',
       '--report', work / 'edited-report.json', '--author', 'libreoffice', '--id', 'site/hero/edited')
    saved = work / 'saved'
    saved.mkdir()
    run(['soffice', '-env:UserInstallation=' + (work / 'lo-profile').as_uri(), '--headless',
         '--convert-to', 'docx', '--outdir', saved, FIXTURE])
    resaved = saved / FIXTURE.name
    assert resaved.exists(), 'LibreOffice must produce a real re-save'
    ox('docx-import', PROJECT, REPORT, resaved, '--out', work / 'resaved.json',
       '--report', work / 'resaved-report.json', '--author', 'libreoffice', '--id', 'site/hero/resaved')
    edited_report = json.loads((work / 'edited-report.json').read_text())
    saved_report = json.loads((work / 'resaved-report.json').read_text())
    assert edited_report['changed'] and edited_report['conflicts'], 'Choose a fixture with text changes and a cell conflict'
    store = work / 'project'
    ox('init', store, '--from', PROJECT)
    ox('key', 'new', 'fixture-reviewer', store)
    ox('commit', store, work / 'resaved.json', '--sign', '--key', 'fixture-reviewer')
    # Verify all layers under the default signature policy, not just a painted check.
    for layer in (store / '.ox/layers').glob('*.json'):
        ox('sign', store, layer.stem, '--key', 'fixture-reviewer')
    verification = ox('verify', store, '--require')
    signed_layer = next(p for p in (store / '.ox/layers').glob('*.json')
                        if json.loads(p.read_text())['header']['id'] == 'site/hero/resaved')
    signature = next(json.loads(p.read_text()) for p in (store / '.ox/sigs' / signed_layer.stem).glob('*.json')
                     if not json.loads(p.read_text()).get('alg'))
    original_blocks = document(original)
    def frame(label, app, path, report):
        blocks = document(path)
        old_p = [b['text'] for b in original_blocks if b['kind'] == 'paragraph']
        new_p = [b['text'] for b in blocks if b['kind'] == 'paragraph']
        return {'label': label, 'app': app, 'sha256': digest(path.read_bytes()), 'blocks': blocks,
                'paragraphDiffs': [segments(old_p[i] if i < len(old_p) else '', text) for i, text in enumerate(new_p)],
                'rewrites': rewrites(original, path), 'report': report}
    # A real Calc edit feeds two other app writers in the same project.
    flow_store = work / 'flow-project'
    ox('init', flow_store, '--from', PROJECT)
    flow_before = flow_snapshot(flow_store, work, 'flow-before')
    workbook_layer = work / 'workbook-edit.json'
    workbook_report = work / 'workbook-report.json'
    ox('xlsx-import', PROJECT, WORKBOOK, EXAMPLE / 'fixtures/finance.libreoffice-edited.xlsx',
       '--out', workbook_layer, '--report', workbook_report,
       '--author', 'libreoffice', '--id', 'site/hero/workbook')
    ox('commit', flow_store, workbook_layer)
    flow_after = flow_snapshot(flow_store, work, 'flow-after')
    assert flow_before['source']['rows'] != flow_after['source']['rows'], 'Workbook must really change'
    flow = {'fixture': 'examples/hello-table/fixtures/finance.libreoffice-edited.xlsx',
            'route': 'Calc workbook edit → WordCraft report file + DeckCraft deck',
            'before': flow_before, 'after': flow_after,
            'importReport': json.loads(workbook_report.read_text()),
            'scope': 'Content previews extracted from real exports, not app screenshots. '
                     'The report has a linked table; the deck has the linked table and chart.'}
    result = {
        'flow': flow,
        'generatedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'fixture': 'examples/hello-table/fixtures/report.libreoffice-edited.docx',
        'baseline': 'examples/hello-table/project.ox.json',
        'commands': commands,
        'frames': [frame('Original', 'Milleleaf / WordCraft', original, {}),
                   frame('Edited', 'LibreOffice fixture', FIXTURE, edited_report),
                   frame('Re-saved', 'LibreOffice on the box', resaved, saved_report)],
        'signed': {'signer': signature['signer'], 'at': signature['at'], 'layer': signature['layer'],
                   'signature': signature, 'verification': verification, 'exitCode': 0,
                   'scope': 'Fresh fixture test key. Build-time verification, not a trusted time-stamp or approval.'},
        'rawLayer': json.loads(signed_layer.read_text()),
        'layerBytes': signed_layer.read_text(),
    }
    target = ROOT / 'site/src/data/hero.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(result, indent=2, ensure_ascii=False) + '\n')
    public = ROOT / 'site/public/evidence/hero.json'
    public.parent.mkdir(parents=True, exist_ok=True)
    public.write_text(target.read_text())
    print('Hero evidence: actual edited report, real re-save, signed layer; ox verify --require exit 0.')
