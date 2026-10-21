#!/usr/bin/env python3
"""Generate authored RFC9842 fixtures with independent official C encoders.

Build google/brotli commit028fb5a23661f123017c060daa546b55cf4bde29's CLI,
then pass --brotli, --zstd-library and --zstd-include. The C producer proves
that decoding without the supplied dictionary does not recover the payload.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--brotli", type=Path, required=True)
parser.add_argument("--zstd-library", type=Path, required=True)
parser.add_argument("--zstd-include", type=Path, required=True)
args = parser.parse_args()
directory = Path(__file__).parent / "fixtures" / "compression-dictionary"
dictionary = b"".join(base64.b64encode(hashlib.sha256(f"authored-dictionary-{i}".encode()).digest()) + b"\n" for i in range(1024))
payload = b"<main>" + dictionary[5000:21000] + b"|authored delta|" + dictionary[25000:34000] + b"</main>"
digest = hashlib.sha256(dictionary).digest()
with tempfile.TemporaryDirectory(prefix="surf-dictionary-oracle-") as temporary:
    temporary = Path(temporary)
    (temporary / "dictionary").write_bytes(dictionary)
    (temporary / "payload").write_bytes(payload)
    subprocess.run([str(args.brotli.resolve()), "-q", "9", "-w", "24", "-D", str(temporary / "dictionary"), "-o", str(temporary / "encoded.br"), str(temporary / "payload")], check=True)
    subprocess.run([str(args.brotli.resolve()), "-q", "9", "--large_window=25", "-D", str(temporary / "dictionary"), "-o", str(temporary / "oversized.br"), str(temporary / "payload")], check=True)
    ordinary = subprocess.run([str(args.brotli.resolve()), "-d", "-o", str(temporary / "without"), str(temporary / "encoded.br")], capture_output=True)
    if ordinary.returncode == 0 and (temporary / "without").read_bytes() == payload:
        raise RuntimeError("Brotli fixture did not actually require its dictionary")
    subprocess.run(["clang", "-O2", "-I", str(args.zstd_include), str(directory / "zstd-encoder.c"), str(args.zstd_library), "-o", str(temporary / "zstd-encoder")], check=True)
    zstd_version = subprocess.check_output([str(temporary / "zstd-encoder"), str(temporary / "dictionary"), str(temporary / "payload"), str(temporary / "encoded.zstd")], text=True).strip()
    contents = {
        "dictionary.bin": dictionary,
        "decoded.bin": payload,
        "encoded.dcb": bytes.fromhex("ff444342") + digest + (temporary / "encoded.br").read_bytes(),
        "encoded.dcz": bytes.fromhex("5e2a4d1820000000") + digest + (temporary / "encoded.zstd").read_bytes(),
        "oversized-window.dcb": bytes.fromhex("ff444342") + digest + (temporary / "oversized.br").read_bytes(),
    }
    for name, body in contents.items():
        (directory / name).write_bytes(body)
    provenance = {
        "schemaVersion": 1,
        "brotliCommit": "028fb5a23661f123017c060daa546b55cf4bde29",
        "brotliVersion": subprocess.check_output([str(args.brotli.resolve()), "--version"], text=True).strip(),
        "zstdVersion": zstd_version,
        "zstdSource": "https://github.com/facebook/zstd/releases/tag/v1.5.7",
        "dictionaryRequiredWithoutDictionary": {"dcb": True, "dcz": True},
        "generatorSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "zstdProducerSha256": hashlib.sha256((directory / "zstd-encoder.c").read_bytes()).hexdigest(),
        "files": {name: {"bytes": len(body), "sha256": hashlib.sha256(body).hexdigest()} for name, body in contents.items()},
    }
    (directory / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
