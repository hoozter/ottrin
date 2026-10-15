import os,pathlib,subprocess
root=pathlib.Path.cwd();out=root/".hermes/evidence/copy-payload-current"
stage=root/".hermes/packaging-tmp/linux-current"
assert not stage.exists(), "Preserve unexpected staging"
stage.mkdir(parents=True)
env=os.environ.copy();env.update(OTTRIN_SKIP_BUILD="1",OTTRIN_DIST_DIR=str(stage),APPIMAGE_EXTRACT_AND_RUN="1",PATH=str(root/".hermes/tools/rpm/usr/bin")+":"+env["PATH"],RPM_CONFIGDIR=str(root/".hermes/tools/rpm/usr/lib/rpm"),LD_LIBRARY_PATH=str(root/".hermes/tools/rpm/usr/lib/x86_64-linux-gnu"))
for kind in ("appimage","portable","deb","rpm"):
 print("Packaging "+kind,flush=True)
 with (out/(kind+"-build.log")).open("w") as log:subprocess.run(["bash",str(root/("packaging/linux/package-"+kind+".sh"))],env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
print("Packages staged",flush=True)
