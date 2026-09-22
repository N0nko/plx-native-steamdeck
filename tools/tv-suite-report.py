#!/usr/bin/env python3
"""Build the three current TV galleries and keep untested features visible."""
import argparse
import html
import json
from pathlib import Path

GROUPS = ['1-home-movies-tv', '2-search-discover', '3-playback-details-watchlist']
STYLE = '<style>body{background:#14161c;color:#eef;font:16px system-ui;margin:32px}a{color:#cef}img{width:100%;border-radius:10px}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(440px,1fr));gap:24px}article{background:#23262e;padding:16px;border-radius:12px}td,th{text-align:left;padding:10px;border-bottom:1px solid #445}small{color:#bcc}</style>'


def build(root, catalog):
    known = {c['id'] for c in catalog}
    summary = []
    for group in GROUPS:
        folder = root / group
        folder.mkdir(parents=True, exist_ok=True)
        cards, seen, builds = [], {}, set()
        for path in sorted(folder.rglob('manifest.json')):
            report = json.loads(path.read_text(encoding='utf-8'))
            revision = report['revision']
            builds.add(revision)
            for row in report['steps']:
                image = path.parent / (row['name'] + '.png')
                if not image.exists():
                    continue
                relative = image.relative_to(folder).as_posix()
                label = html.escape(row['name'])
                status = html.escape(row['status'])
                cards.append(f'<article><h2>{label}</h2><a href="{relative}"><img loading="lazy" src="{relative}"></a><p>{status}</p><p>{html.escape(row.get("review", ""))}</p><small>{revision[:8]} · {html.escape(row["observed"])}</small></article>')
                for feature in row.get('covers', []):
                    if feature not in known:
                        raise ValueError('Unknown coverage ID: ' + feature)
                    seen.setdefault(feature, []).append(row)
        coverage = []
        for feature in (c for c in catalog if c['group'] == group):
            evidence = seen.get(feature['id'], [])
            status = 'Not exercised — needs a fixture/run'
            if evidence:
                status = 'Captured; route checked; visual/behavior review required' if all('MISMATCH' not in r['status'] for r in evidence) else 'Route mismatch — inspect partial run'
            coverage.append(dict(feature, status=status, captures=len(evidence)))
        (folder / 'coverage.json').write_text(json.dumps(coverage, indent=2), encoding='utf-8')
        rows = ''.join(f'<tr><td>{html.escape(c["title"])}</td><td>{html.escape(c["status"])}</td></tr>' for c in coverage)
        (folder / 'index.html').write_text('<!doctype html><meta charset="utf-8">' + STYLE + f'<h1>{group}</h1><p>Every capture carries its build. Missing evidence stays visible; menu inspection does not prove the action works.</p><table>{rows}</table><main>' + ''.join(cards) + '</main>', encoding='utf-8')
        summary.append(f'<li><a href="{group}/index.html">{group}</a> — {len(cards)} captures, {len(coverage)} feature checks</li>')
    (root / 'UI-GALLERY.html').write_text('<!doctype html><meta charset="utf-8">' + STYLE + '<h1>PlxNative current TV suite</h1><ul>' + ''.join(summary) + '</ul><p>Older sweeps are in archive/previous-sweeps. Add coverage IDs and a fixture to tests/tv whenever a feature changes.</p>', encoding='utf-8')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    source = Path(__file__).resolve().parent.parent
    build(args.output, json.loads((source / 'tests/tv/coverage.json').read_text(encoding='utf-8')))
