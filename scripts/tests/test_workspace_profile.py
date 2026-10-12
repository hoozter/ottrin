import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.name == "posix", "Native Windows runner acceptance is separate")
class WorkspaceProfileTests(unittest.TestCase):
    def exercise(self, mode):
        with tempfile.TemporaryDirectory(prefix="ottrin-test-runner-contract-") as temporary:
            fixture = Path(temporary)
            binary_dir = fixture / "bin"
            binary_dir.mkdir()
            cargo = binary_dir / "cargo"
            cargo.write_text(f"#!{sys.executable}\n" + """
import json,os,pathlib,subprocess,sys,time
state=pathlib.Path(os.environ['XDG_STATE_HOME'])
(state/'test-owned').write_text('disposable state')
child=None
unit=None
if sys.argv[-1]=='cancel':
 child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(1000)'])
if sys.argv[-1]=='managed-cancel':
 unit='ottrin-render-testprofile-'+str(os.getpid())+'.service'
 subprocess.run(['systemd-run','--user','--collect','--quiet','--unit='+unit,
                 '--property=TimeoutStopSec=2',sys.executable,'-c','import time;time.sleep(1000)',str(state)],check=True)
 (state/'.ottrin-stdout').write_text('test-owned renderer marker')
pathlib.Path(os.environ['TEST_RECEIPT']).write_text(json.dumps({
 'state':str(state),'home':os.environ['HOME'],'cargo':os.environ['CARGO_HOME'],
 'rustup':os.environ['RUSTUP_HOME'],'args':sys.argv[1:],
 'runtime':os.environ.get('XDG_RUNTIME_DIR'),'bus':os.environ.get('DBUS_SESSION_BUS_ADDRESS'),
 'child':child.pid if child else None,'unit':unit}))
if unit:
 while True:time.sleep(1)
if child:child.wait()
sys.exit(23 if sys.argv[-1]=='failure' else 0)
""")
            cargo.chmod(0o700)
            operator_state = fixture / "operator-state"
            operator_state.mkdir()
            sentinel = operator_state / "preserve"
            sentinel.write_bytes(b"OPERATOR STATE MUST REMAIN")
            receipt_path = fixture / "receipt.json"
            env = os.environ.copy()
            env.update(PATH=str(binary_dir) + os.pathsep + env["PATH"],
                       XDG_STATE_HOME=str(operator_state), TEST_RECEIPT=str(receipt_path),
                       XDG_RUNTIME_DIR=str(fixture / "session-runtime"),
                       DBUS_SESSION_BUS_ADDRESS="unix:path=/test-owned-manager",
                       CARGO_HOME=str(fixture / "shared-cargo"),
                       RUSTUP_HOME=str(fixture / "shared-rustup"))
            if mode == "managed-cancel":
                for variable in ("DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR"):
                    if variable in os.environ:env[variable] = os.environ[variable]
                    else:env.pop(variable, None)
            process = subprocess.Popen(
                [sys.executable, str(ROOT / "scripts/test-workspace.py"), mode], env=env
            )
            try:
                deadline = time.monotonic() + 10
                while not receipt_path.exists() and time.monotonic() < deadline:
                    if process.poll() is not None:
                        self.fail("runner exited without exercising Cargo")
                    time.sleep(0.02)
                self.assertTrue(receipt_path.exists())
                receipt = json.loads(receipt_path.read_text())
                if mode in ("cancel", "managed-cancel"):
                    process.send_signal(signal.SIGTERM)
                code = process.wait(timeout=10)
                self.assertEqual(code, {"success": 0, "failure": 23, "cancel": 143, "managed-cancel": 143}[mode])
                self.assertNotEqual(receipt["state"], str(operator_state))
                self.assertFalse(Path(receipt["state"]).parent.exists())
                self.assertFalse(Path(receipt["home"]).exists())
                self.assertEqual(receipt["cargo"], env["CARGO_HOME"])
                self.assertEqual(receipt["rustup"], env["RUSTUP_HOME"])
                self.assertEqual(receipt["runtime"], env.get("XDG_RUNTIME_DIR"))
                self.assertEqual(receipt["bus"], env.get("DBUS_SESSION_BUS_ADDRESS"))
                self.assertEqual(receipt["args"], ["test", mode])
                self.assertEqual(sentinel.read_bytes(), b"OPERATOR STATE MUST REMAIN")
                self.assertEqual(list(operator_state.iterdir()), [sentinel])
                if receipt["unit"]:
                    active = subprocess.run(["systemctl", "--user", "is-active", receipt["unit"]], capture_output=True)
                    self.assertNotEqual(active.returncode, 0)
                if receipt["child"]:
                    status = Path("/proc") / str(receipt["child"]) / "status"
                    self.assertTrue(not status.exists() or "State:\tZ" in status.read_text())
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=10)
                if receipt_path.exists():
                    unit = json.loads(receipt_path.read_text()).get("unit")
                    if unit:
                        subprocess.run(["systemctl", "--user", "stop", unit], capture_output=True)


    def test_success_preserves_operator_state_and_shared_build_cache(self):
        self.exercise("success")

    def test_failure_preserves_exit_code_and_removes_private_state(self):
        self.exercise("failure")

    def test_cancellation_stops_children_and_removes_private_state(self):
        self.exercise("cancel")

    @unittest.skipUnless(sys.platform == "linux", "systemd renderer cleanup is Linux-specific")
    def test_cancellation_stops_only_renderer_units_using_the_private_profile(self):
        manager = subprocess.run(["systemctl", "--user", "show", "--property=Version", "--value"], capture_output=True)
        if manager.returncode:self.skipTest("user-systemd manager unavailable")
        control = f"ottrin-render-profile-control-{os.getpid()}.service"
        subprocess.run(["systemd-run", "--user", "--collect", "--quiet", "--unit=" + control,
                        "--property=TimeoutStopSec=2", "/usr/bin/sleep", "1000"], check=True)
        try:
            self.exercise("managed-cancel")
            active = subprocess.run(["systemctl", "--user", "is-active", control], capture_output=True)
            self.assertEqual(active.returncode, 0, "unrelated renderer control must remain running")
        finally:
            subprocess.run(["systemctl", "--user", "stop", control], check=True)



if __name__ == "__main__":
    unittest.main()
