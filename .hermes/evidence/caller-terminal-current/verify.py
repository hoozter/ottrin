"""Bind the caller terminal-recovery source/build to native behavior and owned teardown."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import zipfile

r = Path(__file__).resolve().parent
e = r / "native"


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read(path):
    return json.loads(path.read_text())


s = read(r / "SOURCE_INPUT.json")
b = read(r / "NATIVE_TEST_INPUT.json")
n = read(e / "APPLICATION_INPUT.json")
assert b["source"] == s
assert n["changed_product_sources"] == s["files"]
assert n["source_revision"] == s["reference_commit"]
for path, expected in s["files"].items():
    assert digest(path) == expected, path
for artifact in b["artifacts"]:
    assert digest(artifact["executable"]) == artifact["sha256"]
artifact = next(a for a in b["artifacts"] if a["crate"] == "ottrin_platform")
assert (n["source_test_sha256"], n["source_bytes"]) == (artifact["sha256"], artifact["bytes"])
for name, key in [("actor.ps1", "actor_sha256"), ("system-companion.ps1", "system_companion_sha256")]:
    assert digest(r / name) == n[key]

archive = e / "RESULTS.zip"
with zipfile.ZipFile(archive) as z:
    members = sorted(z.namelist())
    assert members == sorted(["CALLER_READY.json", "DESKTOP_READY.json", "OWNER.json", "OWNER_OBSERVATION.json", "RESULT.json", "SYSTEM_PROCESS.json", "SYSTEM_RESULT.json"])
    records = {Path(name).stem: json.loads(z.read(name).decode("utf-8-sig")) for name in members}
for name, value in records.items():
    (e / ("GUEST_OWNER.json" if name == "OWNER" else name + ".json")).write_text(json.dumps(value, indent=2) + "\n")
g = read(e / "GUEST_RESULT.json")
system = records["SYSTEM_RESULT"]
assert g == records["RESULT"] and g["status"].startswith("pass")
assert system["status"].startswith("pass") and g["system"] == system
assert len(g["runs"]) == 1 and g["runs"][0]["case"] == "ordinary cross-principal client"
plan = read(r / "REGRESSION_PLAN.json")
assert [x["case"] for x in system["runs"]] == ["caller private helper"] + [x["name"] for x in plan["tests"]]
runs = g["runs"] + system["runs"]
assert len(runs) == plan["expected_top_level_invocations"] == 13
for run in runs:
    assert run["exit"] == 0 and not run.get("timed_out", False), run
    assert re.findall(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored", run["stdout"])[-1] == ("1", "0"), run
for run, test in zip(system["runs"][1:], plan["tests"]):
    assert "test " + test["full_name"] + " ... " in run["stdout"], test
helper = system["runs"][0]["stdout"]
assert "caller public dispatcher provisioning: cancellation, relative path and source overlap refuse before writes; creation, public Undo and cleanup preserve existing profile data" in helper
for directory in ["false", "true"]:
    for case in range(4):
        assert f"caller public dispatcher source_directory={directory} case={case}: original source pin, conflict policy, complete payload/metadata, receipt refresh, idempotent Undo and cleanup passed" in helper
    assert f"caller public cleanup source_directory={directory}: single-member admission" in helper
    for interrupted in ["false", "true"]:
        assert f"caller public cohort source_directory={directory} interrupted={interrupted}: canonical admission" in helper
        assert f"caller public shared cohort source_directory={directory} interrupted={interrupted}: canonical admission" in helper
    for undo in ["false", "true"]:
        assert f"caller cleanup discovery source_directory={directory} undo={undo}: readonly payload/metadata status" in helper

for directory in ["false", "true"]:
    for undo in ["false", "true"]:
        assert f"caller terminal name release source_directory={directory} undo={undo}: readonly admission, cancellation, exclusive edited payload preservation, stale refusal and released-name reuse passed" in helper
    for overwrite in ["false", "true"]:
        for mode in range(3):
            assert f"caller end attempt source_directory={directory} overwrite={overwrite} mode={mode}: caller admission, cancellation, exclusive unknown payload/original preservation, prefix-safe name release and stale refusal passed" in helper

owner = read(e / "OWNER.json")
assert all(owner["cleanup"][key] for key in ["domain_removed", "fixture_removed", "base_still_off"])
assert owner["cleanup"]["retained_pending_volume"] is None
for pid in [owner["private_console"]["pid"], owner.get("desktop_observer_pid")]:
    if pid:
        assert not Path("/proc", str(pid)).exists(), pid
assert not Path(owner["private_console"]["home"]).parent.exists()
domains = subprocess.check_output(["virsh", "-c", "qemu:///system", "list", "--all"], text=True)
volumes = subprocess.check_output(["virsh", "-c", "qemu:///system", "vol-list", "default"], text=True)
assert owner["domain"] not in domains + volumes
value = {
    "status": "pass caller end-attempt/name-release admissions and affected ordinary controls",
    "source": "SOURCE_INPUT.json", "build": "NATIVE_TEST_INPUT.json",
    "native_invocations": 13, "top_level_test_passes": 13,
    "caller_terminal_name_release_cases": 4, "caller_end_attempt_cases": 12,
    "caller_dispatch_conflict_cases": 8, "caller_skip_without_receipt_cases": 2,
    "caller_conflict_receipt_undo_cleanup_cases": 6, "caller_first_provisioning_case": 1,
    "ordinary_tests": plan["tests"], "archive_sha256": digest(archive),
    "archive_members": members, "unchanged_executable_text": n["unchanged_executable_text"],
    "coverage": [
        "End-attempt and name-release plans retain explicit runtime Authority and the full original canonical registry Binding; ordinary wrappers select Process once",
        "Caller registry exposes read-only admission for both plans; mutation reopens the exact binding, uses caller-aware native-ID parent resolution and preserves existing control identity/security/content checks",
        "Four file/directory Undone/CleaningUp cases preserve later edited/unknown payloads under an exclusive root lease, leave both journals byte-identical, refuse cancelled/stale/repeated admission and permit only confirmed target-name reuse",
        "Twelve file/directory creation/overwrite cases preserve unverified edited/unknown payloads and existing public originals under exclusive payload leases; direct completion releases names, complete paired prefixes recover through fresh same-client contexts and divergent prefixes refuse recovery/reuse without repair",
        "Wrong Process authority and noncanonical caller registries refuse; original sources/preferences/index bytes, IDs and security remain unchanged; all preceding caller security/recovery/cohort/dispatcher controls still run",
        "All eleven ordinary abort/name-release/claim-repair/dependency parent tests run with retained exact stdout"
    ],
    "lint": {"command": "cargo clippy -p ottrin-platform -p ottrin-ui --target x86_64-pc-windows-gnu --all-targets -- -D warnings", "exit": 0, "sha256": digest(r / "windows-lint.log")},
    "independent_teardown": {"domains": domains, "volumes": volumes, "console_observer_profile_absent": True},
    "limits": [
        "Native engine recovery APIs are covered; full helper command/history/recovery routing and advertised administrator Copy remain disabled",
        "Fresh contexts of the same live client do not establish real client/helper restart or newly authenticated connections",
        "Complete caller mutation-prefix, readonly/symbolic-link publication, original-retention/restoration failures, in-place TokenOwner, alternate-admin UAC and package/profile contexts remain required",
        "Stable application packages are unchanged; hosted CI acceptance remains separate"
    ]
}
(r / "ACCEPTANCE.json").write_text(json.dumps(value, indent=2) + "\n")
print("PASS13 native invocations; four caller terminal name-release and12 end-attempt cases; independent teardown verified.")
