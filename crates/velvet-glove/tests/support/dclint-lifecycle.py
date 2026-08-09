import json
import os
import signal
import sys
import time


MODE = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_MODE", "clean")
INVOKED = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_INVOKED")
SOURCE_CONFIG = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_SOURCE_CONFIG")
ORPHAN_PID = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_ORPHAN_PID")
ORPHAN_LATE = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_ORPHAN_LATE")
UNSELECTED = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_UNSELECTED")
RETAINED_DIRECTORY = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_DIRECTORY")
CONFIG_CAPTURE = os.environ.get("VELVET_GLOVE_DCLINT_LIFECYCLE_CONFIG_CAPTURE")


def record_invocation():
    if INVOKED:
        with open(INVOKED, "a", encoding="utf-8") as output:
            output.write(json.dumps(sys.argv[1:], separators=(",", ":")) + "\n")


def selected_files():
    return [argument for argument in sys.argv[1:] if not argument.startswith("--")]


def config_path():
    argument = next(
        item for item in sys.argv[1:] if item.startswith("--config=")
    )
    return argument.removeprefix("--config=")


def message(rule, fixable, kind="warning"):
    return {
        "rule": rule,
        "type": kind,
        "category": "style",
        "severity": "minor" if kind == "warning" else "critical",
        "message": f"lifecycle diagnostic for {rule}",
        "line": 1,
        "column": 1,
        "fixable": fixable,
        "data": {},
    }


def report_record(path, messages):
    return {
        "filePath": path,
        "messages": messages,
        "errorCount": sum(item["type"] == "error" for item in messages),
        "warningCount": sum(item["type"] == "warning" for item in messages),
        "fixableErrorCount": sum(
            item["type"] == "error" and item["fixable"] for item in messages
        ),
        "fixableWarningCount": sum(
            item["type"] == "warning" and item["fixable"] for item in messages
        ),
    }


def emit(records, status):
    json.dump(records, sys.stdout, separators=(",", ":"))
    sys.stdout.write("\n")
    raise SystemExit(status)


def cancellation_probe():
    descendant_pid_path = os.environ["VELVET_GLOVE_DCLINT_LIFECYCLE_DESCENDANT_PID"]
    leader_pid_path = os.environ["VELVET_GLOVE_DCLINT_LIFECYCLE_CHILD_PID"]
    ready_path = os.environ["VELVET_GLOVE_DCLINT_LIFECYCLE_READY"]
    descendant = os.fork()
    if descendant == 0:
        for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, signal.SIG_IGN)
        while True:
            time.sleep(1)
    for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, signal.SIG_IGN)
    with open(leader_pid_path, "w", encoding="ascii") as output:
        output.write(f"{os.getpid()}\n")
    with open(descendant_pid_path, "w", encoding="ascii") as output:
        output.write(f"{descendant}\n")
    with open(ready_path, "w", encoding="ascii") as output:
        output.write("ready\n")
    while True:
        time.sleep(1)


def normal_exit_orphan(close_stdio):
    descendant = os.fork()
    if descendant == 0:
        for signum in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, signal.SIG_IGN)
        if close_stdio:
            os.close(sys.stdout.fileno())
            os.close(sys.stderr.fileno())
        time.sleep(0.75)
        with open(ORPHAN_LATE, "w", encoding="ascii") as output:
            output.write("late mutation\n")
        while True:
            time.sleep(1)
    with open(ORPHAN_PID, "w", encoding="ascii") as output:
        output.write(f"{descendant}\n")


record_invocation()
files = selected_files()
fix = "--fix" in sys.argv[1:]

if MODE == "cancel":
    cancellation_probe()
if MODE == "output-cap":
    chunk = b"x" * 65536
    try:
        for _ in range(600):
            os.write(sys.stdout.fileno(), chunk)
    except BrokenPipeError:
        pass
    raise SystemExit(0)
if MODE == "native-stderr":
    sys.stderr.write("native lifecycle failure with unstable/path/detail\n")
    raise SystemExit(1)
if MODE == "malformed-json":
    sys.stdout.write("not json\n")
    raise SystemExit(1)
if MODE == "non-finite-json":
    record = report_record(
        files[0], [message("services-alphabetical-order", True, "warning")]
    )
    raw = json.dumps([record], separators=(",", ":"))
    raw = raw.replace('"data":{}', '"data":{"overflow":1e999}', 1)
    sys.stdout.write(raw + "\n")
    raise SystemExit(1)
if MODE == "config-swap":
    with open(SOURCE_CONFIG, "wb") as output:
        output.write(b'{"quiet":true}\n')
    emit([report_record(path, []) for path in files], 0)
if MODE == "capture-config":
    with open(config_path(), "rb") as source:
        contents = source.read()
    with open(CONFIG_CAPTURE, "wb") as output:
        output.write(contents)
    emit([report_record(path, []) for path in files], 0)
if MODE == "private-config-destroy":
    private = config_path()
    parent = os.path.dirname(private)
    os.unlink(private)
    os.rmdir(parent)
    with open(parent, "wb") as output:
        output.write(b"not a directory\n")
    emit([report_record(path, []) for path in files], 0)
if MODE in {"normal-exit-orphan-closed", "normal-exit-orphan-pipe"}:
    normal_exit_orphan(MODE == "normal-exit-orphan-closed")
    emit([report_record(path, []) for path in files], 0)

if MODE in {"invalid-yaml", "invalid-schema", "unknown-error"}:
    emit(
        [report_record(path, [message(MODE, False, "error")]) for path in files],
        1,
    )
if MODE == "unfixable":
    emit(
        [
            report_record(
                path,
                [message("service-image-require-explicit-tag", False, "error")],
            )
            for path in files
        ],
        1,
    )
if MODE == "ambiguous-fixability":
    emit(
        [
            report_record(
                path,
                [message("services-alphabetical-order", False, "warning")],
            )
            for path in files
        ],
        1,
    )
if MODE == "unknown-rule":
    emit(
        [report_record(path, [message("future-fixable-rule", True)]) for path in files],
        1,
    )

if MODE in {
    "fixable",
    "noop-fix",
    "touch-only",
    "partial-failure",
    "mutate-clean",
    "unselected-change",
    "unselected-create",
    "unselected-delete",
    "directory-add",
    "directory-remove",
    "directory-chmod",
}:
    if fix:
        if MODE == "partial-failure":
            with open(files[0], "wb") as output:
                output.write(b"partial\n")
            sys.stderr.write("partial write failure\n")
            raise SystemExit(1)
        if MODE == "touch-only":
            info = os.stat(files[0])
            os.utime(
                files[0], ns=(info.st_atime_ns, info.st_mtime_ns + 1_000_000_000)
            )
        elif MODE != "noop-fix":
            for path in files:
                with open(path, "wb") as output:
                    output.write(b"fixed\n")
        if MODE == "mutate-clean":
            with open(
                os.environ["VELVET_GLOVE_DCLINT_LIFECYCLE_SELECTED_CLEAN"], "wb"
            ) as output:
                output.write(b"corrupted\n")
        if MODE == "unselected-change":
            with open(UNSELECTED, "wb") as output:
                output.write(b"corrupted unselected\n")
        elif MODE == "unselected-create":
            with open(UNSELECTED + ".created", "wb") as output:
                output.write(b"created out of scope\n")
        elif MODE == "unselected-delete":
            os.unlink(UNSELECTED)
        elif MODE == "directory-add":
            os.mkdir(RETAINED_DIRECTORY + ".created", 0o711)
        elif MODE == "directory-remove":
            os.rmdir(RETAINED_DIRECTORY)
        elif MODE == "directory-chmod":
            os.chmod(RETAINED_DIRECTORY, 0)
        emit([report_record(path, []) for path in files], 0)
    records = []
    issues = False
    for path in files:
        with open(path, "rb") as source:
            dirty = source.read() == b"dirty\n"
        messages = (
            [message("services-alphabetical-order", True, "warning")]
            if dirty
            else []
        )
        issues = issues or bool(messages)
        records.append(report_record(path, messages))
    emit(records, 1 if issues else 0)

emit([report_record(path, []) for path in files], 0)
