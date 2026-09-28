"""Render the fixture harness's JSON report as a GitHub job summary."""

import collections
import html
import json
import pathlib
import sys

REPORT_FORMAT = 2


def first_lines(detail, limit=300):
    lines = [line.strip() for line in detail.splitlines() if line.strip()]
    return " ".join(lines[:3])[:limit]


def render(root):
    print("## Real-tool fixtures\n")
    versions = root / "versions.txt"
    if versions.exists():
        print("<pre>" + html.escape(versions.read_text()) + "</pre>\n")
    reports = sorted(root.glob("report-*.json"))
    if len(reports) != 1:
        print("No unique completed fixture report. Check setup, build, and probe logs.")
        return 1
    report = json.loads(reports[0].read_text())
    if report.get("formatVersion") != REPORT_FORMAT:
        print(f'Unsupported report formatVersion {report.get("formatVersion")!r}.')
        return 1
    surfaces = report["surfaces"]
    counts = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
    excluded = set()
    reasons = collections.Counter()
    failures = []
    for outcome in report["outcomes"]:
        reason = outcome.get("reason") or {}
        if outcome["status"] == "skip" and reason.get("code") == "not-selected":
            excluded.add(outcome["tool"])
            continue
        counts[outcome["tool"]][outcome["surface"]][outcome["status"]] += 1
        if outcome["status"] == "skip":
            reasons[f'{outcome["tool"]}: {reason["detail"]}'] += 1
        elif outcome["status"] == "fail":
            failures.append(
                f'{outcome["tool"]}/{outcome["case"]} ({outcome["surface"]}, '
                f'expected {outcome["expected"]}): {first_lines(reason.get("detail", ""))}'
            )
    print(
        "Cells are passed / failed / skipped cases per surface. Deferred surfaces run "
        "session-start-state → post-tool → turn-completion and compare summary.json "
        "per-file statuses; immediate surfaces check post-tool-immediate output shape. "
        "Every surface checks post-run file content.\n"
    )
    print("| Tool | " + " | ".join(surfaces) + " |")
    print("| --- |" + " ---: |" * len(surfaces))
    for tool, by_surface in sorted(counts.items()):
        cells = []
        for surface in surfaces:
            totals = by_surface[surface]
            cells.append(f'{totals["pass"]} / {totals["fail"]} / {totals["skip"]}')
        print(f"| {tool} | " + " | ".join(cells) + " |")
    print(f'\nProtocol probe commands: {report["totals"]["probeCommandsExecuted"]}.')
    if failures:
        print("\n### Failures\n")
        for failure in failures:
            print(f"- {html.escape(failure)}")
    if reasons:
        print("\nSkip reasons:\n")
        for reason, count in sorted(reasons.items()):
            print(f"- {html.escape(reason)} ({count} surfaces)")
    print(f"\n{len(excluded)} fixture tools outside this job's selection; no validation claimed.")
    if excluded:
        print("\n<details><summary>Tools not selected</summary>\n")
        print(", ".join(sorted(excluded)))
        print("\n</details>")
    print("\nDownload the job artifact for JSON results, versions, logs, and failure workspaces.")
    return int(report["totals"]["failed"] > 0 or report["totals"]["attemptedSurfaceCases"] == 0)


if __name__ == "__main__":
    sys.exit(render(pathlib.Path(sys.argv[1])))
