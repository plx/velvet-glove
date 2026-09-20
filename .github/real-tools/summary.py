"""Render the fixture harness's JSON report as a GitHub job summary."""

import collections
import html
import json
import pathlib
import sys


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
    counts = collections.defaultdict(collections.Counter)
    excluded = set()
    reasons = collections.Counter()
    for outcome in report["outcomes"]:
        reason = outcome.get("reason") or {}
        if outcome["status"] == "skip" and reason.get("code") == "not-selected":
            excluded.add(outcome["tool"])
            continue
        counts[outcome["tool"]][outcome["status"]] += 1
        if outcome["status"] == "skip":
            reasons[f'{outcome["tool"]}: {reason["detail"]}'] += 1
    print("Counts are fixture cases × Claude/Codex surfaces.\n")
    print("| Tool | Passed | Failed | Skipped |\n| --- | ---: | ---: | ---: |")
    for tool, totals in sorted(counts.items()):
        print(f'| {tool} | {totals["pass"]} | {totals["fail"]} | {totals["skip"]} |')
    print(f'\nProtocol probe commands: {report["totals"]["probeCommandsExecuted"]}.')
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
