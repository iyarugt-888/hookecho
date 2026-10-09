"""Write docs/certification/README.md from manifest.json (run manifest.py first)."""
import json, pathlib

cert = pathlib.Path('docs/certification')
m = json.loads((cert / 'manifest.json').read_text(encoding='utf-8'))

TASKS = {
    'M1.1': 'A5', 'M1.2': 'A3', 'M1.3': 'A1, A2', 'M1.4': 'A1, A4',
    'M2.1': 'B1', 'M2.2': 'B1', 'M2.3': 'B2, H3', 'M2.4': 'B3',
    'M3.1': 'C3', 'M3.2': 'C4', 'M3.3': 'C4', 'M3.4': 'C1', 'M3.5': 'C2', 'M3.6': 'C5',
    'M4.1': 'D3', 'M4.2': 'D1', 'M4.3': 'D2', 'M4.4': 'D3',
    'M5.1': 'E1', 'M5.2': 'E2', 'M5.3': 'E3', 'M5.4': 'E2, E4',
    'M6.2': 'F1', 'M6.3': 'F2', 'M6.4': 'F3', 'M7.1': 'H2', 'M7.2': 'H1', 'M7.3': 'H4',
}

rows = []
for c in m['cards']:
    ev = f"[{c['card'].lower()}/]({c['card'].lower()}/)" if c['evidence_dir'] else '—'
    rows.append(
        f"| {c['card']} | {c['title']} | {TASKS.get(c['card'], '—')} | {c['ledger_entries']} | "
        f"{c['latest_entry'] or '—'} | {ev} |"
    )

why = '\n'.join(f'- {w}' for w in m['why_not_certified'])
text = f"""# Certification evidence (ROADMAP_PARITY M7.3, 1008.md H4)

**Not a release certification.** This index gathers the evidence reviewed so far. Release
readiness is **not claimed**, for these reasons:

{why}

[manifest.json](manifest.json) (schema {m['schema']}) lists every tracked evidence file under
this folder with its SHA-256 and size. It was generated at {m['generated_utc']} from commit
`{m['generated_from_commit'][:12]}` on `{m['branch']}`. Measurement host, unless a file says
otherwise: {m['measurement_host']}. {m['hash_basis'][0].upper() + m['hash_basis'][1:]}.

Regenerate it after adding evidence:

```bash
python scripts/certification/manifest.py
python scripts/certification/readme.py
```

## Cards

The ledger count (dated entries, counted by their headings) and latest date come from each card's evidence ledger in `ROADMAP_PARITY.md`,
which holds the commands, results and open items. Every card's Android gate is open unless its
ledger records a physical-device run.

| Card | Title | 1008.md | Ledger entries | Latest | Evidence |
| --- | --- | --- | --- | --- | --- |
{chr(10).join(rows)}

## Operator guides

| Guide | Covers |
| --- | --- |
| [GUIDE.md](../GUIDE.md) | Using the app |
| [TROUBLESHOOTING.md](../TROUBLESHOOTING.md) | Recovering failed workflows and stale data |
| [DATA.md](../DATA.md) | Data sources |
| [time-alignment.md](../time-alignment.md) | How layers are matched in time |
| [obs-guide.md](../obs-guide.md) | The output window with OBS |
| [spotter-network.md](../spotter-network.md) | Why reports are not submitted to Spotter Network |

These guides have not been reviewed against every workflow added in this round. That review is
part of the open M7.3 work.
"""
(cert / 'README.md').write_text(text, encoding='utf-8', newline='')
print('ok', len(rows))
