"""Assess the matched actual AppImage idle workflows and reviewed originals."""
from pathlib import Path
import hashlib
import json

e = Path(__file__).resolve().parent
read = lambda p: json.loads(p.read_text())
cases = {}
for name in ("baseline", "candidate"):
    root = e / ("linux-" + name)
    control = read(root / "CONTROL.json")
    result = read(root / "FUNCTIONAL_RESULT.json")
    teardown = read(root / "TEARDOWN.json")
    assert result["status"].startswith("pass native idle workflow")
    assert result["idle"]["seconds"] >= 30
    assert result["idle"]["no_external_probe_clients_or_injected_input_during_sample"]
    assert result["index_counts"] == [1, 2, 1]
    assert result["source_and_preferences_preserved"] and result["normal_exit"] == 0
    for key in ("index_before", "index_after"):
        assert result[key]["status"] == "ready"
        assert result[key]["watcher_ready"] and result[key]["indexed_items"] == 1
    assert teardown["status"] == "closed" and teardown["profile_removed"]
    assert not Path(teardown["profile"]).exists()
    assert hashlib.sha256((root / "WORKER_SOURCE.py").read_bytes()).hexdigest() == control["worker_sha256"]
    captures = []
    for filename, review in (
        ("main-idle.png", "Reviewed original: one-file list, intact themed frame and controls; no error overlay."),
        ("index-updated.png", "Reviewed original: Settings Search health visibly shows 2 files indexed and active watcher, consistent themed navigation/cards."),
    ):
        p = root / filename
        captures.append({"path": str(p.relative_to(e)), "sha256": hashlib.sha256(p.read_bytes()).hexdigest(), "review": review})
    cases[name] = {"source_revision": control["source_revision"], "artifact_sha256": control["artifact_sha256"], "worker_sha256": control["worker_sha256"], "idle": result["idle"], "index_counts": result["index_counts"], "captures": captures, "teardown": teardown}
assert cases["candidate"]["source_revision"] == read(e / "BUILD_SOURCE.json")["source_revision"]
assert cases["baseline"]["worker_sha256"] == cases["candidate"]["worker_sha256"]
assert cases["candidate"]["idle"]["cpu_one_core_percent"] < 2
assert cases["candidate"]["idle"]["cpu_one_core_percent"] < cases["baseline"]["idle"]["cpu_one_core_percent"]
receipt = {"source_revision": cases["candidate"]["source_revision"], "status": "pass matched Linux AppImage idle and native index observation", "scope": "Private X11, Dark150, one-file corpus, 5-second warmup and 30-second GUI-process CPU sample. Linux uses Glow with LIBGL_ALWAYS_SOFTWARE=1; the matched Vulkan ICD input was unused (no Vulkan driver maps). Native Search count follows external create/delete without injected input. No sustained, personal-service, physical graphics, storage insertion or whole-appearance acceptance.", "cases": cases}
(e / "LINUX_NATIVE_ACCEPTANCE.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(receipt["status"])
