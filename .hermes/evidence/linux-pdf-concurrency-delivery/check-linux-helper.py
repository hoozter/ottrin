"""Actual unprivileged Linux helper protocol5 contract; no filesystem action."""
from pathlib import Path
import hashlib
import argparse
import importlib.util
import json
import subprocess
import sys

sys.dont_write_bytecode = True
root = Path.cwd()
r = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("test_workspace", root / "scripts/test-workspace.py")
workspace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workspace)
parser = argparse.ArgumentParser()
parser.add_argument("--helper", type=Path, required=True)
args = parser.parse_args()
helper = args.helper.resolve(strict=True)
with helper.open("rb") as stream:
    digest = hashlib.file_digest(stream, "sha256").hexdigest()
cases = []
with workspace.isolated_environment(prefix="ottrin-helper-protocol-") as (profile, environment):
    def run(arguments, payload=None):
        result = subprocess.run([str(helper), *arguments], input=None if payload is None else json.dumps(payload), env=environment, cwd=profile, capture_output=True, text=True, timeout=5)
        assert not [path for path in profile.rglob("*") if path.is_file()], "Protocol probe created app data"
        return result

    for arguments, expected in [
        (["--check-protocol", "5"], 0),
        (["--check-protocol", "5", "--copy-recovery"], 0),
        (["--check-protocol", "4", "--copy-recovery"], 78),
        (["--check-protocol", "3", "--copy-recovery"], 78),
        (["--check-protocol", "5", "--bound-local-pipe"], 69),
    ]:
        result = run(arguments)
        assert result.returncode == expected and not result.stdout and not result.stderr, result
        cases.append({"arguments": arguments, "exit": result.returncode})

    for action in ["Resume", "EndAttempt", "ReleaseName"]:
        request = {"protocol_version": 5, "command": {"RecoverCopy": {"transaction": "a" * 32, "action": action}}, "context": {"reason": "Owned protocol contract probe", "cwd": None}}
        result = run(["--stdio-json"], request)
        response = json.loads(result.stdout)
        assert result.returncode == 0 and response["status"] == "Unsupported" and response["payload"] is None, response
        cases.append({"action": action, "exit": result.returncode, "response": response})

    for version, action in [(4, "Resume"), (5, "UnrecognizedAction")]:
        result = run(["--stdio-json"], {"protocol_version": version, "command": {"RecoverCopy": {"transaction": "a" * 32, "action": action}}, "context": {"reason": None, "cwd": None}})
        response = json.loads(result.stdout)
        assert result.returncode == 0 and response["status"] == "Failed" and response["payload"] is None, response
        cases.append({"version": version, "action": action, "exit": result.returncode, "response": response})
    profile_path = str(profile)
assert not Path(profile_path).exists()
value = {"status": "pass actual Linux helper protocol5", "executable": str(helper), "sha256": digest, "source": "BUILD_SOURCE.json", "cases": cases, "profile_removed": profile_path, "created_app_files": 0, "scope": "Actual unprivileged executable negotiation, old-protocol/unknown-action refusal and unsupported Windows-only recovery. Bundled package helper only; no pkexec, installed-helper or protected-file acceptance."}
(r / "LINUX_HELPER_RUNTIME.json").write_text(json.dumps(value, indent=2) + "\n")
print("PASS10 actual bundled Linux helper protocol cases; disposable profile removed; no app files created")
