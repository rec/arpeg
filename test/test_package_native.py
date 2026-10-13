from hashlib import sha256
from pathlib import Path
from zipfile import ZipFile

import pytest

from arpeg.profile import parse_profile
from tools.package_native import PackageNative


@pytest.mark.parametrize("binary_name", ["arpeg", "arpeg.exe"])
def test_native_bundle_keeps_executable_profiles_and_matching_checksum(
    tmp_path: Path, binary_name: str
) -> None:
    binary = tmp_path / binary_name
    binary.write_bytes(b"standalone executable")
    binary.chmod(0o755)
    output = tmp_path / "download"
    PackageNative(
        version="v0.2.0", target="test-target", binary=binary, output=output
    ).run()
    archive = output / "arpeg-v0.2.0-test-target.zip"
    prefix = "arpeg-v0.2.0-test-target"
    with ZipFile(archive) as bundle:
        assert len(bundle.namelist()) == 5
        executable = bundle.getinfo(f"{prefix}/{binary_name}")
        assert bundle.read(executable) == binary.read_bytes()
        assert executable.external_attr >> 16 & 0o777 == binary.stat().st_mode & 0o777
        for name in ("up", "live-latch", "history-wind"):
            profile = parse_profile(
                bundle.read(f"{prefix}/profiles/{name}.toml").decode()
            )
            assert profile.name == name
    assert archive.with_suffix(".zip.sha256").read_text() == (
        f"{sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n"
    )
