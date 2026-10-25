#!/usr/bin/env python3
"""Write the Tauri updater manifest (latest.json) for a release (T41, docs/release.md).

The app fetches https://github.com/<repo>/releases/latest/download/latest.json, checks
`version` against its own, and downloads `url` after verifying `signature` with the public key
in tauri.conf.json. Apple Silicon only (ADR-0001), so there is one platform entry.
"""
import argparse
import datetime
import json
import pathlib


def manifest(version: str, repo: str, tarball: str, signature: str, notes: str, now: datetime.datetime) -> dict:
    return {
        "version": version,
        "notes": notes,
        "pub_date": now.strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {
            "darwin-aarch64": {
                "signature": signature.strip(),
                "url": f"https://github.com/{repo}/releases/download/v{version}/{tarball}",
            }
        },
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--version", required=True)
    ap.add_argument("--repo", required=True)
    ap.add_argument("--tarball", required=True, help="file name of the .app.tar.gz release asset")
    ap.add_argument("--sig", required=True, help="path to the .sig written by `tauri signer sign`")
    ap.add_argument("--notes", default="")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    notes = a.notes or f"https://github.com/{a.repo}/releases/tag/v{a.version}"
    data = manifest(a.version, a.repo, a.tarball, pathlib.Path(a.sig).read_text(), notes,
                    datetime.datetime.now(datetime.timezone.utc))
    pathlib.Path(a.out).write_text(json.dumps(data, indent=2) + "\n")
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
