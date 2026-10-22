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
    result = {
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
