#!/usr/bin/env python3
"""Verify the pinned, unchanged native audio source and adapted FFT bytes."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[2]
vendor = root / "rust/crates/turbo-surf-render/native/audio"
manifest = json.loads((vendor / "PROVENANCE.json").read_text())
for row in manifest["originalFiles"]:
    path = vendor / row["path"]
    if hashlib.sha256(path.read_bytes()).hexdigest() != row["sha256"]:
        raise SystemExit(f"Pinned audio source changed: {row['path']}")
count=len(manifest["originalFiles"])
for row in manifest["portedSources"]:
    if "storedPath" not in row:
        continue
    if hashlib.sha256((vendor/row["storedPath"]).read_bytes()).hexdigest()!=row["sha256"]:
        raise SystemExit(f"Pinned audio reference changed: {row['storedPath']}")
    count+=1
for row in json.loads((vendor/"effects/PROVENANCE.json").read_text()):
    if hashlib.sha256((vendor/"effects/upstream"/row["path"]).read_bytes()).hexdigest()!=row["sha256"]:
        raise SystemExit(f"Pinned effect source changed: {row['path']}")
    count+=1
fft = root / "rust/crates/turbo-surf-render/src/audio_fft.rs"
if hashlib.sha256(fft.read_bytes()).hexdigest() != manifest["rustfft"]["adaptedSha256"]:
    raise SystemExit("Adapted Chromium RustFFT source changed")
print(f"Verified {count} exact Chrome {manifest['chromeVersion']} native audio files and adapted RustFFT")
