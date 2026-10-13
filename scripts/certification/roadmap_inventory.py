"""Preserve the pre-consolidation plans and emit conservative migration dispositions.

Run once to capture, subsequently without overwriting frozen source bytes. This is
a source-to-workstream inventory, not a test runner or automatic acceptance gate.
All unchecked/mixed requirements retain an open parent disposition unless a
reviewed override below explains their replacement. Explicit checkboxes are kept
separately so an old checkbox cannot disappear inside a section summary.
"""
import collections
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[2]
ARCHIVE = ROOT / "roadmaps/archive"
SOURCES = [
    "ROADMAP.md", "ROADMAP_NEW.md", "ROADMAP_2.md", "ROADMAP_PARITY.md",
    "CODEX_GOAL.md", "suggestions.md", "newsuggestions.md", "detectionplan.md",
    "3d_rework.md", "1008.md", "1010.md", "theme_plan.md", "expansion.md",
    "docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md",
]
PARTIAL = "partially implemented"
CERT = "certification-only"
SUPER = "superseded"
NEEDED = "still needed"
DONE = "completed"

EVIDENCE = {
    "live-radar": "docs/certification/m1.3/README.md",
    "radar-analysis": "docs/certification/m3.3/README.md",
    "storm-analysis": "docs/certification/m2.2/README.md",
    "models-satellite": "docs/certification/m5.1/README.md",
    "gis": "docs/certification/m4.4/README.md",
    "broadcast": "docs/certification/m6.3/README.md",
    "mobile": "android/README.md",
    "certification": "docs/certification/README.md",
}
EVIDENCE["live-radar"] = "docs/certification/m1.3/continuation.md"


def lane_for(source, heading):
    """Route historical task families, preserving their original identifiers."""
    if source == "ROADMAP_PARITY.md":
        m = re.search(r"\bM([0-7])\.\d", heading)
        if m:
            return ["certification", "live-radar", "storm-analysis", "radar-analysis",
                    "gis", "models-satellite", "broadcast", "certification"][int(m[1])]
    if source == "ROADMAP_NEW.md":
        m = re.match(r"([A-R])\d", heading)
        if m:
            return {"A": "models-satellite", "B": "live-radar", "C": "radar-analysis",
                    "D": "models-satellite", "E": "models-satellite", "F": "models-satellite",
                    "G": "models-satellite", "H": "radar-analysis", "I": "gis",
                    "J": "models-satellite", "K": "certification", "L": "storm-analysis",
                    "M": "broadcast", "N": "live-radar", "O": "certification",
                    "P": "certification", "Q": "mobile", "R": "storm-analysis"}[m[1]]
    if source == "1008.md":
        m = re.match(r"([A-H])\d", heading)
        if m:
            return dict(A="live-radar", B="storm-analysis", C="radar-analysis", D="gis",
                        E="models-satellite", F="broadcast", G="certification", H="mobile")[m[1]]
    if source == "1010.md":
        m = re.match(r"V(\d+)", heading)
        if m:
            return {1: "mobile", 2: "mobile", 3: "mobile", 4: "mobile", 5: "certification",
                    6: "live-radar", 7: "live-radar", 8: "storm-analysis", 9: "storm-analysis",
                    10: "models-satellite", 11: "mobile", 12: "models-satellite",
                    13: "certification", 14: "storm-analysis", 15: "models-satellite",
                    16: "models-satellite", 17: "radar-analysis", 18: "radar-analysis",
                    19: "radar-analysis", 20: "certification"}[int(m[1])]
    s = heading.lower()
    if source == "3d_rework.md":
        return "radar-analysis"
    if source == "detectionplan.md":
        return "certification"
    for pattern, lane in [
        (r"android|mobile|tablet|touch|accessib|phone|ios", "mobile"),
        (r"gis|shapefile|geojson|kml|projection handling", "gis"),
        (r"broadcast|output|scene|capture|obs |export", "broadcast"),
        (r"latency|live|failover|progressive|source.health|vcp|sails|provenance", "live-radar"),
        (r"3d|gate|beam|trail|column|radar.analysis|user.defined|user product|cross.section|udp", "radar-analysis"),
        (r"storm|route|chase|impact|motion|feature track|warning", "storm-analysis"),
        (r"model|goes|satellite|ensemble|field|mrms|timeline|pane|workspace|rtma", "models-satellite"),
    ]:
        if re.search(pattern, s):
            return lane
    return "certification"


def disposition(source, heading, body):
    """Reviewed family rules: never infer completion from a checked box alone."""
    lane = lane_for(source, heading)
    status = PARTIAL
    basis = "Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open."
    h = heading.lower()
    if re.search(r"sequence|implementation order|immediate.*task|ranking|lanes|phase order", h):
        status, basis = SUPER, "Old execution ordering replaced by roadmaps/README.md priorities and simultaneous Tracks A–E."
    elif re.search(r"acceptance|definition of done|release gate|scorecard|test strategy|verification matrix|performance.*target|performance budget|on.device|long.run profile", h):
        status, basis = CERT, "Preserved acceptance obligation; execute against the implemented slice and keep missing evidence open. Harness gaps are tracked separately in E1/E4."
    elif source == "newsuggestions.md" and re.match(r"S\d+\.", heading):
        status, basis = DONE, "The source's final implementation ledger records S1–S10 shipped; corroborated by current GIS, action, probe, comparison and health paths. New parent operational/device gates remain separate."
    elif source == "3d_rework.md":
        status, basis = SUPER, "Original rendering design implemented and measured in its Verification table; follow-on grid/ROI work superseded its old remainder (M3.6). Android proof remains R6/M1."
    elif source == "detectionplan.md":
        status, basis = PARTIAL, "Claude-owned scientific program; progress records completed phases and rejected experiments, while calibration/QLCS/held-out limits remain. Preserve every experiment and do not rerun rejected approaches as new feature tasks."
        if re.match(r"Phase (?:0|1|2|3|4|5|6|7|10|11|12|13)\b", heading):
            status, basis = SUPER, "Original phase specification superseded by the detailed Progress experiments/results in this source; maintain scientific regressions in Track D, with no new detector implementation assigned here."
    elif source in ("theme_plan.md", "docs/WSV3_IMGUI_MODERN_DESIGN_PLAN.md"):
        status, basis = SUPER, "Later workstation chrome and recorded browser/desktop layout walkthroughs replace the original design sequence. Outstanding physical-device/input/label work remains M1/M4/G2."
    if re.search(r"not planned|what not to do|out of scope|deliberately not suggested", h):
        status, basis = SUPER, "Preserved scope exclusion; not a ready feature task. New priorities retain the no-breadth/no-new-platform boundary."
    if source == "ROADMAP_NEW.md":
        key = heading.split(".")[0]
        if key in {"G2", "G3", "G4", "E7", "P1", "P2", "P3", "R2"}:
            status, basis = NEEDED, "Deferred extension/research requirement, not a demonstrated higher-priority reliability gap. Re-audit before implementation; do not expand ahead of the current queue."
        if key in {"A1", "A2", "A3", "D2", "D3", "E2", "E3", "E4", "G1", "J6", "N2", "N4"}:
            status, basis = SUPER, "The original foundation request has been implemented according to its own dated checks and current consumers. Remaining workflow/platform acceptance is tracked in the destination; do not rebuild the foundation."
        if key in {"Q4", "Q5"}:
            status, basis = SUPER, "Additional platform expansion remains outside the current competitive reliability queue; preserve existing platform behavior."
    if source == "1010.md":
        if re.match(r"V(?:1|2|3|6|8|14|15)\.", heading):
            status, basis = SUPER, "Request overtaken by later code/commits and certification ledgers (audit.md); retain remaining operational/platform proof in the destination."
        if re.match(r"E[1-7]\.", heading):
            status, basis = CERT, "Evidence track runs alongside implementation. Existing renderer soak results are retained; operational/device/presentation proof is not inferred."
    if source == "1008.md" and re.match(r"G[1-3]\.", heading):
        status, basis = SUPER, "Experiments evaluated in detectionplan.md and not adopted; scientific owner retains the results/limits. No repeated unbacktested implementation."
    return lane, status, basis


def collect(source, text):
    lines = text.splitlines()
    starts = []
    fence = False
    for i, line in enumerate(lines):
        if line.lstrip().startswith("```"):
            fence = not fence
        if fence:
            continue
        match = re.match(r"^#{1,6}\s+(.+)", line)
        task = re.match(r"^\*\*((?:[A-H]\d|V\d+|E\d)\. .+?)\*\*", line)
        if match or task:
            starts.append((i, (match or task)[1]))
    records = []
    for n, (start, heading) in enumerate(starts):
        end = starts[n + 1][0] if n + 1 < len(starts) else len(lines)
        body = "\n".join(lines[start:end])
        # Inherit task family across descriptive subheadings, keeping each section addressable.
        family = heading
        for _, parent in reversed(starts[:n + 1]):
            if re.match(r"(?:M\d\.\d|[A-R]\d+(?:\.\d+)?|V\d+)\b", parent):
                family = parent
                break
        lane, status, basis = disposition(source, family, body)
        record = dict(source=source, line=start + 1, end_line=end, title=heading,
                      family=family, status=status, workstream=lane, basis=basis,
                      evidence=EVIDENCE[lane], kind="section")
        records.append(record)
        for offset, line in enumerate(lines[start:end]):
            m = re.match(r"^\s*[-*]\s+\[([ xX])\]\s+(.*)", line)
            if m:
                # Parent classification is deliberately conservative: checked is a historical
                # assertion, not evidence that physical/operational acceptance has happened.
                records.append(dict(record, line=start + offset + 1,
                                    title=m[2], kind="checkbox", historical_checked=m[1] != " "))
    return records


def main():
    ARCHIVE.mkdir(parents=True, exist_ok=True)
    (ARCHIVE / "sources").mkdir(exist_ok=True)
    (ARCHIVE / "items").mkdir(exist_ok=True)
    manifest_path = ARCHIVE / "source-manifest.json"
    if manifest_path.exists():
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    else:
        manifest = {
            "schema": 1,
            "baseline_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
            "basis": "Exact working-tree bytes before consolidation; includes pre-existing peer changes. Not all bytes belong to baseline_commit.",
            "initial_status": subprocess.check_output(["git", "status", "--short"], cwd=ROOT, text=True),
            "sources": [],
        }
        for source in SOURCES:
            raw = (ROOT / source).read_bytes()
            dest = ARCHIVE / "sources" / Path(source).name
            dest.write_bytes(raw)
            manifest["sources"].append(dict(path=source, snapshot=dest.relative_to(ROOT).as_posix(),
                                            sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw)))
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    records = []
    index = ["# Historical item dispositions", "",
             "Every heading section and explicit checkbox is retained below. A section includes",
             "its ordinary bullet/numbered requirements and prose. Checkbox rows inherit the",
             "reviewed family disposition; a historical check is not a certification verdict.",
             "**Partially implemented** is conservative for mixed/uncertain requirements, not",
             "a claim that each subordinate requirement has code. Use the live workstream's",
             "bounded tasks before choosing new work. Source copies are immutable; new results",
             "go in the live workstream and completed-increments ledger.", "",
             "| Source | Sections | Checkboxes | Item ledger |", "| --- | --- | --- | --- |"]
    for source_info in manifest["sources"]:
        source = source_info["path"]
        raw = (ROOT / source_info["snapshot"]).read_bytes()
        if hashlib.sha256(raw).hexdigest() != source_info["sha256"]:
            raise ValueError(f"Frozen source changed: {source}")
        items = collect(source, raw.decode("utf-8-sig"))
        records.extend(items)
        name = Path(source).stem + ".md"
        index.append(f"| `{source}` | {sum(i['kind'] == 'section' for i in items)} | {sum(i['kind'] == 'checkbox' for i in items)} | [dispositions](items/{name}) |")
        out = [f"# {source} — migration dispositions", "",
               f"[Original bytes](../sources/{Path(source).name}) · [current plan](../../README.md)", "",
               "Rows name original line numbers. Section rows include their subordinate prose/list",
               "requirements. Evidence is historical; statuses do not claim fresh tests.", "",
               "| Line | Item | Status | Destination / evidence |", "| --- | --- | --- | --- |"]
        for item in items:
            title = item["title"].replace("|", "&#124;")
            if item["kind"] == "checkbox":
                title = ("[x] " if item["historical_checked"] else "[ ] ") + title
            out.append(f"| {item['line']} | {title} | {item['status']} | [{item['workstream']}](../../{item['workstream']}/README.md); [evidence](../../../{item['evidence']}) |")
        out.extend(["", "Classification bases (by family):", ""])
        bases = dict.fromkeys((i["family"], i["basis"]) for i in items if i["kind"] == "section")
        out.extend(f"- **{family}**: {basis}" for family, basis in bases)
        (ARCHIVE / "items" / name).write_text("\n".join(out) + "\n", encoding="utf-8")
    counts = collections.Counter(i["status"] for i in records)
    index.extend(["", "Inventory counts (includes parent and checkbox rows, not unique features):", ""])
    index.extend(f"- {status}: {count}" for status, count in sorted(counts.items()))
    (ARCHIVE / "inventory.md").write_text("\n".join(index) + "\n", encoding="utf-8")
    (ARCHIVE / "items.json").write_text(json.dumps(records, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"{len(manifest['sources'])} frozen sources; {len(records)} classified section/checkbox records: {dict(counts)}")


if __name__ == "__main__":
    main()
