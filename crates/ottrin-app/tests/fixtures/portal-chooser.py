#!/usr/bin/python3
"""Immutable child for real portal pipe-close and pinned-pidfd tests."""
import json
import os
from pathlib import Path
import sys
import time

assert sys.argv[1:] == ["picker", "--cancel-on-disconnect"]
request = json.loads(sys.stdin.readline())
directory = request["initial_directory"]
assert directory["encoding"] == "unix-bytes"
root = Path(os.fsdecode(bytes.fromhex(directory["units_hex"])))
assert root.is_absolute()
if request["title"] == "graceful":
    sys.stdin.read()
    (root / "graceful").write_text("closed")
    print(json.dumps(dict(accepted=False, paths=[], current_filter=None, choices={})))
elif request["title"] == "unresponsive":
    while True:
        time.sleep(30)
else:
    raise ValueError("Unknown owned portal fixture mode")
