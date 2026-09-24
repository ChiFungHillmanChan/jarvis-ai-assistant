#!/usr/bin/env python3
"""Launch the real Tauri desktop app with an isolated, reusable QA database."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--reuse", type=Path, help="Reuse a QA directory printed by an earlier run")
parser.add_argument("--no-build", action="store_true", help="Launch the previously built QA app")
parser.add_argument("--first-run", action="store_true", help="Show first-launch language and setup screens in a new QA directory")
parser.add_argument("--presentation", action="store_true", help="Seed a labelled synthetic analysis for native rendering checks")
args = parser.parse_args()
if args.first_run and args.reuse:
    parser.error("--first-run requires a new QA directory; omit --reuse")
root = Path(__file__).resolve().parent
if args.reuse:
    directory = args.reuse.resolve()
    if not (directory / ".jarvis-native-qa").is_file():
        parser.error("--reuse requires a directory created by this script")
else:
    directory = Path(tempfile.mkdtemp(prefix="jarvis-native-qa-"))
    (directory / ".jarvis-native-qa").touch(mode=0o600)

# Keep the host's platform/build environment, never inherit API keys or tokens.
allowed = {"PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "LC_ALL",
           "TERM", "SDKROOT", "DEVELOPER_DIR", "RUSTUP_HOME", "CARGO_HOME"}
env = {key: value for key, value in os.environ.items() if key in allowed}
env["JARVIS_NATIVE_QA_DIR"] = str(directory)
if args.first_run:
    env["JARVIS_NATIVE_QA_FIRST_RUN"] = "1"
if args.presentation:
    env["JARVIS_NATIVE_QA_PRESENTATION"] = "1"
env["RUST_LOG"] = "warn"
print(f"Native QA database: {directory / 'jarvis.db'}", flush=True)
print(f"Restart with: python3 native-mail-qa.py --reuse '{directory}'", flush=True)
config = json.dumps({
    "productName": "JARVIS Native QA", "identifier": "com.jarvis.nativeqa",
    "bundle": {"createUpdaterArtifacts": False, "macOS": {"signingIdentity": "-"}},
})
app = root / "src-tauri/target/debug/bundle/macos/JARVIS Native QA.app"
if not args.no_build:
    subprocess.run(["npm", "run", "tauri", "--", "build", "--debug", "--features", "native-qa",
                    "--bundles", "app", "--config", config], cwd=root, env=env, check=True)
elif not (app / "Contents/MacOS/jarvis").is_file():
    parser.error("Build the QA app once before using --no-build")
print(f"Native QA app: {app}", flush=True)
raise SystemExit(subprocess.call([str(app / "Contents/MacOS/jarvis")], cwd=directory, env=env))
