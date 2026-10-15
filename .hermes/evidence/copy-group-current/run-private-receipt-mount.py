import os,pathlib,signal,subprocess,sys
root=pathlib.Path.cwd();out=root/".hermes/evidence/copy-group-current"
executable=root/"target/debug/deps/ottrin_platform-ae68b2dc04dbe44d"
env=os.environ.copy();env["OTTRIN_QA_PARENT_MOUNT_NAMESPACE"]=os.readlink("/proc/self/ns/mnt")
for key,suffix in [("HOME","home"),("XDG_CONFIG_HOME","config"),("XDG_DATA_HOME","data"),("XDG_STATE_HOME","state"),("XDG_CACHE_HOME","cache"),("XDG_RUNTIME_DIR","runtime")]:env[key]="/tmp/ottrin-owned-receipt-mount/"+suffix
parent=os.getpid()
code="import ctypes,os,sys;libc=ctypes.CDLL(None);assert libc.prctl(1,9)==0;assert os.getppid()==int(sys.argv[1]);os.execve(sys.argv[2],[sys.argv[2],'--exact','tests::copy_receipt_does_not_pin_mount_and_refuses_after_unmount_and_replacement','--ignored','--nocapture'],os.environ)"
process=None
def cancel(signum,frame):raise KeyboardInterrupt()
signal.signal(signal.SIGTERM,cancel)
try:
 with (out/"private-receipt-mount.log").open("w") as log:
  process=subprocess.Popen(["unshare","--user","--map-root-user","--mount","--propagation","private",sys.executable,"-c",code,str(parent),str(executable)],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  assert process.wait(timeout=30)==0, "See private-receipt-mount.log"
finally:
 if process and process.poll() is None:
  os.killpg(process.pid,signal.SIGKILL);process.wait(timeout=5)
