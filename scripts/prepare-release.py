#!/usr/bin/env python3
"""Prepare public release metadata from a completed universal macOS build."""
import argparse
import hashlib
import json
import re
import shutil
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "DwennK/Tasks"


def prepare(bundle: Path, output: Path, tag: str, notes: str):
    version = json.loads((ROOT / "package.json").read_text())["version"]
    config_version = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text())["version"]
    cargo_version = re.search(r'^version = "([^"]+)"', (ROOT / "src-tauri/Cargo.toml").read_text(), re.M)[1]
    if tag != f"v{version}" or config_version != version or cargo_version != version:
        raise ValueError("Tag, package.json, Cargo.toml and tauri.conf.json versions must match")
    archives = list(bundle.glob("macos/*.app.tar.gz"))
    dmgs = list(bundle.glob("dmg/*_universal.dmg"))
    if len(archives) != 1 or len(dmgs) != 1:
        raise ValueError("Expected one signed updater archive and one universal DMG")
    signature = Path(str(archives[0]) + ".sig").read_text().strip()
    if not signature:
        raise ValueError("Missing updater signature")
    output.mkdir(parents=True, exist_ok=True)
    archive_name = f"Tasks_{version}_universal.app.tar.gz"
    dmg_name = f"Tasks_{version}_universal.dmg"
    shutil.copy2(archives[0], output / archive_name)
    shutil.copy2(dmgs[0], output / dmg_name)
    (output / (archive_name + ".sig")).write_text(signature + "\n")
    base = f"https://github.com/{REPOSITORY}/releases/download/{tag}"
    platform = {"signature": signature, "url": f"{base}/{archive_name}"}
    metadata = {
        "version": version, "notes": notes,
        "pub_date": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "platforms": {"darwin-aarch64": platform, "darwin-x86_64": platform},
    }
    (output / "latest.json").write_text(json.dumps(metadata, indent=2) + "\n")
    checksum = hashlib.sha256((output / dmg_name).read_bytes()).hexdigest()
    cask = f'''cask "tasks" do
  version "{version}"
  sha256 "{checksum}"

  url "https://github.com/{REPOSITORY}/releases/download/v#{{version}}/Tasks_#{{version}}_universal.dmg"
  name "Tasks"
  desc "Local-first task manager with notes, reminders, CLI and MCP"
  homepage "https://github.com/{REPOSITORY}"

  auto_updates true
  depends_on macos: :monterey

  app "Tasks.app"
end
'''
    (output / "tasks.rb").write_text(cask)
    # No zap rule: removing the application must not delete the task database.
    files = sorted(p for p in output.iterdir() if p.is_file() and p.name != "SHA256SUMS")
    (output / "SHA256SUMS").write_text("".join(
        f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in files
    ))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--notes", type=Path, required=True)
    args = parser.parse_args()
    prepare(args.bundle, args.output, args.tag, args.notes.read_text())
