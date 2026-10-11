"""Generate docs/certification/manifest.json: every reviewed evidence file with its hash, and
each roadmap card's ledger count and latest entry date (1008.md H4, ROADMAP_PARITY M7.3).
Run from the repository root after committing evidence. Reads evidence and roadmap from HEAD,
not from staged or uncommitted peer edits in the shared checkout."""
import hashlib, json, pathlib, re, subprocess, datetime

root = pathlib.Path('.')
cert = root / 'docs' / 'certification'
commit = subprocess.run(['git', 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip()
branch = subprocess.run(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], capture_output=True, text=True).stdout.strip()

files = []
# Pin both inventory and bytes to the recorded commit. A shared checkout can contain peer
# edits (and CRLF conversions); those must not be attributed to this commit's evidence.
paths = subprocess.run(['git', 'ls-tree', '-r', '--name-only', commit, '--', cert.as_posix()],
                       capture_output=True, text=True, check=True).stdout.splitlines()
for rel in sorted(paths):
    f = pathlib.Path(rel)
    # The manifest itself and the index written from it are derived, not evidence.
    if f.parent == cert and f.name in ('manifest.json', 'README.md'):
        continue
    b = subprocess.run(['git', 'show', f'{commit}:{rel}'], capture_output=True, check=True).stdout
    files.append({'path': rel, 'sha256': hashlib.sha256(b).hexdigest(), 'bytes': len(b)})

road = subprocess.run(['git', 'show', f'{commit}:ROADMAP_PARITY.md'],
                      capture_output=True, check=True).stdout.decode('utf-8')
heads = [(m.start(), m.group(1), m.group(2)) for m in re.finditer(r'^#### (M\d\.\d) — (.+)$', road, re.M)]
cards = []
for i, (pos, cid, title) in enumerate(heads):
    end = heads[i + 1][0] if i + 1 < len(heads) else road.find('\n## 5.', pos)
    body = road[pos:end]
    # "**Evidence ledger — date", "**Increment 2 — date", "**Spatial increment — date"...
    entries = re.findall(
        r'^\*\*(?:Evidence ledger|[A-Za-z0-9 ]*[Ii]ncrement[^*\n]*?) — (\d{4}-\d{2}-\d{2})',
        body,
        re.M,
    )
    status = re.search(r'\*\*Status:\*\* ([^\n]+)', body)
    d = cert / cid.lower()
    cards.append({
        'card': cid,
        'title': title.strip(),
        'ledger_entries': len(entries),
        'latest_entry': max(entries) if entries else None,
        'status_line': status.group(1).strip() if status else None,
        'evidence_dir': d.as_posix() if any(p.startswith(d.as_posix() + '/') for p in paths) else None,
    })

manifest = {
    'schema': 1,
    'generated_utc': datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat(),
    'generated_from_commit': commit,
    'branch': branch,
    'measurement_host': 'Windows 11 Home 10.0.22631, NVIDIA GeForce RTX 2060 (Vulkan)',
    'hash_basis': 'bytes as stored in the repository; text files have LF line endings there, and a checkout with core.autocrlf may show them with CRLF',
    'release_certified': False,
    'why_not_certified': [
        'No Android device evidence for any card (M7.2 open).',
        'Presentation (scan-out) latency is not observed anywhere (M1.2).',
        'Renderer-only soaks do not certify full application and Android lifecycle profiles (M7.1/M7.2).',
        'Operational independent-provider failover remains unverified; loopback continuation is not certification (M1.3).',
        'Several workflows are shown by unit tests and offscreen captures only, not interactive walkthroughs.',
    ],
    'cards': cards,
    'files': files,
}
(cert / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8', newline='')
print(len(files), 'files,', len(cards), 'cards, from', commit[:8])
