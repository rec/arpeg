"""Package the standalone executable and a few playable profiles."""

from hashlib import file_digest
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

import tyro
from pydantic import BaseModel


class PackageNative(BaseModel, frozen=True):
    version: str
    target: str
    binary: Path
    output: Path

    def run(self) -> None:
        name = f"arpeg-{self.version}-{self.target}"
        self.output.mkdir(parents=True, exist_ok=True)
        archive = self.output / f"{name}.zip"
        root = Path(__file__).resolve().parents[1]
        with ZipFile(archive, "w", compression=ZIP_DEFLATED) as bundle:
            bundle.write(self.binary, f"{name}/{self.binary.name}")
            bundle.write(root / "packaging/README-NATIVE.md", f"{name}/README.md")
            for profile in ("up", "live-latch", "history-wind"):
                bundle.write(
                    root / f"conformance/{profile}.toml",
                    f"{name}/profiles/{profile}.toml",
                )
        with archive.open("rb") as source:
            checksum = file_digest(source, "sha256").hexdigest()
        archive.with_suffix(".zip.sha256").write_text(f"{checksum}  {archive.name}\n")
        print(archive)


if __name__ == "__main__":
    tyro.cli(PackageNative).run()
