"""Build isolated Cargo configurations using workspace platform metadata."""

import argparse
from pathlib import Path
import subprocess
import sys
import tomllib


def main():
    root = Path(__file__).resolve().parent.parent
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    platforms = workspace["metadata"]["platforms"]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("build", "check", "clippy", "test", "doc"))
    parser.add_argument("--platform", choices=("native", *platforms, "all"), default="native")
    parser.add_argument("--package", action="append", help="Limit to a package; repeatable")
    parser.add_argument("--message-format", choices=("human", "json"), default="human")
    parser.add_argument("--unlocked", action="store_true", help="Allow interactive lockfile updates")
    args = parser.parse_args()
    if args.command == "test" and args.platform != "native":
        parser.error("tests run on the native host; use build for embedded targets")
    manifests = [tomllib.loads((root / path / "Cargo.toml").read_text()) for path in workspace["members"]]
    known = {manifest["package"]["name"] for manifest in manifests}
    unknown = set(args.package or ()) - known
    if unknown:
        parser.error(f"unknown packages: {', '.join(sorted(unknown))}")
    selected = [m for m in manifests if not args.package or m["package"]["name"] in args.package]
    choices = ["native", *platforms] if args.platform == "all" else [args.platform]
    failed = False
    for platform in choices:
        # A separate invocation per package also detects features accidentally
        # supplied by an unrelated workspace member.
        for manifest in selected:
            package = manifest["package"]
            settings = package.get("metadata", {}).get("polis", {})
            base = ["cargo", args.command, "-p", package["name"], "--no-default-features"]
            if not args.unlocked:
                base.append("--locked")
            if args.command != "doc":
                base.append(f"--message-format={args.message_format}")
            if platform != "native":
                base += ["--target", platforms[platform]["target"]]
                if settings.get("chip-features"):
                    base += ["--features", platforms[platform]["feature"]]
            if args.command in ("build", "check", "clippy"):
                base.append("--all-targets" if platform == "native" else "--lib")
            if args.command == "doc":
                base.append("--no-deps")
            commands = [base]
            if platform != "native" and args.command == "build":
                for example in settings.get("link-examples", []):
                    commands.append([
                        "cargo", "build", "-p", package["name"], "--no-default-features",
                        "--target", platforms[platform]["target"], "--release",
                        "--features", f'{platforms[platform]["feature"]},link-check',
                        "--example", example, f"--message-format={args.message_format}",
                        *([] if args.unlocked else ["--locked"]),
                    ])
            for command in commands:
                if args.command == "clippy":
                    command += ["--", "-D", "warnings"]
                print(f"[{platform}] {' '.join(command)}", file=sys.stderr, flush=True)
                failed |= subprocess.run(command, cwd=root, check=False).returncode != 0
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
