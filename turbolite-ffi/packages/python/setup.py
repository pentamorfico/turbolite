"""
Build script for the turbolite Python package.

Compiles the Rust loadable extension (turbolite-ffi) with HTTPS support and
copies it into the package directory before installation. The wheel is then
tagged as platform-specific because it bundles a native binary.

Build features: loadable-extension,cli-s3,https,zstd

The Rust toolchain (cargo) must be available on PATH. Install from
https://rustup.rs if needed.
"""

from __future__ import annotations

import os
import platform
import shutil
import subprocess
from pathlib import Path

from setuptools import setup
from setuptools.command.build_py import build_py
from wheel.bdist_wheel import bdist_wheel

# Features compiled into the loadable extension.
_FEATURES = "loadable-extension,cli-s3,https,zstd"


def _build_rust_ext(pkg_dir: Path) -> None:
    """Compile turbolite-ffi and copy the result into pkg_dir."""
    here = Path(__file__).parent.resolve()
    # turbolite-ffi/ crate root is two directories above packages/python/
    ffi_root = (here / "../..").resolve()
    # Workspace root is one level above turbolite-ffi/
    workspace_root = (ffi_root / "..").resolve()
    # Override the author-specific ../cinch-target with a standard path
    # inside the workspace clone so pip install from git works.
    target_dir = workspace_root / "target"

    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)

    print(f"turbolite setup: building Rust extension (features: {_FEATURES})")
    print(f"  crate: {ffi_root}")
    print(f"  target: {target_dir}")

    subprocess.run(
        [
            "cargo", "build", "--release", "--lib",
            "--no-default-features", "--features", _FEATURES,
        ],
        cwd=str(ffi_root),
        env=env,
        check=True,
    )

    system = platform.system()
    if system == "Darwin":
        lib_name = "libturbolite_ffi.dylib"
        out_name = "turbolite.dylib"
    elif system == "Windows":
        lib_name = "turbolite_ffi.dll"
        out_name = "turbolite.dll"
    else:
        lib_name = "libturbolite_ffi.so"
        out_name = "turbolite.so"

    src = target_dir / "release" / lib_name
    dst = pkg_dir / out_name

    if not src.exists():
        raise FileNotFoundError(
            f"turbolite setup: expected built extension at {src}. "
            "Check cargo output above for errors."
        )

    shutil.copy2(str(src), str(dst))
    print(f"turbolite setup: installed extension -> {dst}")


class BuildPyWithRust(build_py):
    """build_py subclass that compiles the Rust extension before packaging."""

    def run(self) -> None:
        here = Path(__file__).parent.resolve()
        _build_rust_ext(here / "turbolite")
        super().run()


class PlatformWheel(bdist_wheel):
    """Mark the wheel as platform-specific (it bundles a native .so/.dylib)."""

    def finalize_options(self) -> None:
        super().finalize_options()
        self.root_is_pure = False

    def get_tag(self):
        _, _, plat = super().get_tag()
        return "py3", "none", plat


setup(
    cmdclass={
        "build_py": BuildPyWithRust,
        "bdist_wheel": PlatformWheel,
    }
)
