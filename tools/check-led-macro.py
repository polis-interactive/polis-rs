"""Build the public macro from a renamed dependency in an isolated no_std caller."""

import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib


def main():
    root = Path(__file__).resolve().parent.parent
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    targets = [None, *(p["target"] for p in workspace["metadata"]["platforms"].values())]
    with tempfile.TemporaryDirectory(prefix="polis-led-macro-") as directory:
        caller = Path(directory)
        (caller / "src").mkdir()
        shutil.copyfile(root / "crates/led-output/tests/fixtures/renamed.rs", caller / "src/lib.rs")
        (caller / "Cargo.toml").write_text(
            '[package]\nname = "led-macro-caller"\nversion = "0.0.0"\n'
            'edition = "2024"\npublish = false\n\n[workspace]\n\n'
            '[dependencies]\nled-api = { package = "polis-led-output", path = '
            + json.dumps(str(root / "crates/led-output")) + ' }\n'
        )
        for target in targets:
            command = [
                "cargo", f"+{toolchain}", "build", "--offline",
                "--manifest-path", str(caller / "Cargo.toml"),
                "--target-dir", str(root / "target/macro-callers"),
            ]
            if target:
                command += ["--target", target]
            subprocess.run(command, check=True, cwd=root)


if __name__ == "__main__":
    main()
