"""Build and package the turbolite native extension.

Runs `cargo build` to produce the platform-specific loadable extension
(.so/.dylib/.dll) and bundles it inside the Python package.  The wheel is
tagged as platform-specific so pip caches and serves the right binary.

Features compiled: loadable-extension,cli-s3,https,zstd
"""

from __future__ import annotations

import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

from setuptools import setup
from setuptools.command.build_py import build_py as _build_py
from wheel.bdist_wheel import bdist_wheel

# setup.py lives in  turbolite-ffi/packages/python/
# turbolite-ffi Cargo workspace is two levels up.
_FFI_DIR = Path(__file__).resolve().parent.parent.parent
_PKG_DIR = Path(__file__).resolve().parent / "turbolite"
_FEATURES = "loadable-extension,cli-s3,https,zstd"


def _ext_filename() -> str:
    s = platform.system()
    if s == "Darwin":
        return "turbolite.dylib"
    if s == "Windows":
        return "turbolite.dll"
    return "turbolite.so"


def _lib_filename() -> str:
    s = platform.system()
    if s == "Darwin":
        return "libturbolite_ffi.dylib"
    if s == "Windows":
        return "turbolite_ffi.dll"
    return "libturbolite_ffi.so"


def _build_rust_ext() -> None:
    """Compile the turbolite loadable extension via cargo."""
    # Override CARGO_TARGET_DIR so we use a local target/ directory instead
    # of the workspace-level ../cinch-target path that is only valid in the
    # original developer environment.
    target_dir = _FFI_DIR / "target"
    env = {**os.environ, "CARGO_TARGET_DIR": str(target_dir)}

    cmd = [
        "cargo", "build",
        "--release", "--lib",
        "--no-default-features",
        "--features", _FEATURES,
        "--manifest-path", str(_FFI_DIR / "Cargo.toml"),
    ]
    print(f"turbolite setup.py: running {' '.join(cmd)}", flush=True)
    subprocess.check_call(cmd, env=env)

    src = target_dir / "release" / _lib_filename()
    if not src.exists():
        raise FileNotFoundError(
            f"cargo build succeeded but output not found at {src}"
        )
    dst = _PKG_DIR / _ext_filename()
    shutil.copy2(str(src), str(dst))
    print(f"turbolite setup.py: installed extension {src.name} → {dst}", flush=True)


class build_py(_build_py):
    """Run cargo build before packaging Python files."""

    def run(self) -> None:
        _build_rust_ext()
        super().run()


class PlatformWheel(bdist_wheel):
    def finalize_options(self) -> None:
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self) -> tuple[str, str, str]:
        _, _, plat = super().get_tag()
        return "py3", "none", plat


setup(cmdclass={"build_py": build_py, "bdist_wheel": PlatformWheel})
