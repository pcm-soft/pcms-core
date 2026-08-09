#!/usr/bin/env python3

"""
PCMS build + QEMU launcher.

Builds the x86_64 UEFI target and launches the resulting EFI binary
under QEMU with OVMF firmware.

This script is intended for development and integration testing only.

It does NOT claim that the resulting image is suitable for medical
equipment, clinical use, or hard real-time certification.

Expected project layout:

    ./Cargo.toml
    ./firmware/OVMF_CODE_4M.fd
    ./firmware/OVMF_VARS_4M.fd
    ./firmware/my_vars.fd

The script creates a temporary EFI System Partition directory:

    build/qemu/esp/EFI/BOOT/BOOTX64.EFI

and starts:

    qemu-system-x86_64

with OVMF.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent

TARGET = "x86_64-unknown-uefi"

FIRMWARE_DIR = ROOT / "firmware"

OVMF_CODE = FIRMWARE_DIR / "OVMF_CODE_4M.fd"
OVMF_VARS = FIRMWARE_DIR / "OVMF_VARS_4M.fd"
MY_VARS = FIRMWARE_DIR / "my_vars.fd"

BUILD_DIR = ROOT / "build" / "qemu"
ESP_DIR = BUILD_DIR / "esp"

EFI_BOOT_DIR = ESP_DIR / "EFI" / "BOOT"
EFI_BINARY = EFI_BOOT_DIR / "BOOTX64.EFI"

DEFAULT_MEMORY = "512M"
DEFAULT_CPUS = "2"


def run(
    command: list[str],
    *,
    cwd: Path = ROOT,
) -> None:
    """Run a command and terminate on failure."""

    print()
    print("+", " ".join(command))
    print()

    result = subprocess.run(command, cwd=cwd)

    if result.returncode != 0:
        raise SystemExit(
            f"\nCommand failed with exit code {result.returncode}: "
            f"{command[0]}"
        )


def require_file(path: Path) -> None:
    """Verify that a required file exists."""

    if not path.is_file():
        raise SystemExit(f"Required file not found: {path}")


def require_command(command: str) -> None:
    """Verify that an executable is available in PATH."""

    if shutil.which(command) is None:
        raise SystemExit(
            f"Required executable '{command}' was not found in PATH."
        )


def check_environment() -> None:
    """Check required development tools and firmware files."""

    require_command("cargo")
    require_command("qemu-system-x86_64")

    require_file(OVMF_CODE)
    require_file(OVMF_VARS)

    # my_vars.fd is optional for the actual QEMU launch because the script
    # creates an isolated writable variables file from OVMF_VARS_4M.fd.
    #
    # We still report its presence because it is part of the PCMS firmware
    # tree and may be useful for project-specific UEFI configuration.
    if MY_VARS.is_file():
        print(f"Found custom variable store: {MY_VARS}")
    else:
        print(f"Warning: optional custom variable store not found: {MY_VARS}")


def clean_build() -> None:
    """Remove generated QEMU build artifacts."""

    if BUILD_DIR.exists():
        print(f"Removing {BUILD_DIR}")
        shutil.rmtree(BUILD_DIR)


def prepare_esp() -> None:
    """Create the UEFI fallback boot directory."""

    EFI_BOOT_DIR.mkdir(parents=True, exist_ok=True)


def cargo_build(release: bool) -> None:
    """Build the complete workspace for x86_64 UEFI."""

    command = [
        "cargo",
        "build",
        "--workspace",
        "--target",
        TARGET,
    ]

    if release:
        command.append("--release")

    run(command)


def locate_efi_binary(release: bool) -> Path:
    """Locate the pcms-core UEFI executable."""

    profile = "release" if release else "debug"

    candidates = [
        ROOT / "target" / TARGET / profile / "pcms-core.efi",
        ROOT / "target" / TARGET / profile / "pcms_core.efi",
        ROOT / "target" / TARGET / profile / "pcms-core",
        ROOT / "target" / TARGET / profile / "pcms_core",
    ]

    for candidate in candidates:
        if candidate.is_file():
            return candidate

    # Cargo projects using a custom binary name may not produce one of the
    # conventional names above. Search the target directory as a fallback.
    target_dir = ROOT / "target" / TARGET / profile

    if target_dir.is_dir():
        for candidate in target_dir.iterdir():
            if candidate.is_file() and candidate.suffix.lower() == ".efi":
                return candidate

    raise SystemExit(
        "Could not find the pcms-core EFI executable.\n"
        f"Searched: {target_dir}"
    )


def install_efi_binary(binary: Path) -> None:
    """Install the kernel as the UEFI fallback bootloader."""

    EFI_BOOT_DIR.mkdir(parents=True, exist_ok=True)

    shutil.copy2(binary, EFI_BINARY)

    print()
    print(f"EFI binary: {binary}")
    print(f"Installed:  {EFI_BINARY}")


def prepare_vars() -> Path:
    """
    Prepare a writable OVMF variable store.

    We never modify the repository copy of OVMF_VARS_4M.fd.
    """

    BUILD_DIR.mkdir(parents=True, exist_ok=True)

    writable_vars = BUILD_DIR / "OVMF_VARS_4M.fd"

    # Prefer the user's custom variable store when it exists.
    #
    # It is copied, never modified in-place.
    source = MY_VARS if MY_VARS.is_file() else OVMF_VARS

    shutil.copy2(source, writable_vars)

    return writable_vars


def launch_qemu(
    *,
    vars_file: Path,
    memory: str,
    cpus: str,
    headless: bool,
) -> None:
    """Launch the PCMS UEFI image in QEMU."""

    command = [
        "qemu-system-x86_64",

        # CPU configuration.
        "-machine",
        "q35",

        "-cpu",
        "max",

        "-m",
        memory,

        "-smp",
        cpus,

        # UEFI firmware.
        "-drive",
        f"if=pflash,format=raw,readonly=on,file={OVMF_CODE}",

        "-drive",
        f"if=pflash,format=raw,file={vars_file}",

        # Expose our development ESP directory as a FAT drive.
        "-drive",
        f"format=raw,file=fat:rw:{ESP_DIR}",

        # Serial console.
        "-serial",
        "stdio",

        # No reboot loop if the firmware/application crashes.
        "-no-reboot",

        # Exit QEMU when the guest powers off.
        "-no-shutdown",
    ]

    if headless:
        command.extend(
            [
                "-display",
                "none",
            ]
        )

    print()
    print("Launching QEMU...")
    print()

    run(command)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Build PCMS and launch it under QEMU/OVMF."
    )

    parser.add_argument(
        "--debug",
        action="store_true",
        help="Build debug profile instead of release.",
    )

    parser.add_argument(
        "--clean",
        action="store_true",
        help="Remove generated QEMU artifacts before building.",
    )

    parser.add_argument(
        "--no-run",
        action="store_true",
        help="Build and prepare the EFI image without launching QEMU.",
    )

    parser.add_argument(
        "--headless",
        action="store_true",
        help="Disable QEMU graphical output and use the serial console.",
    )

    parser.add_argument(
        "--memory",
        default=DEFAULT_MEMORY,
        help=f"QEMU memory size (default: {DEFAULT_MEMORY}).",
    )

    parser.add_argument(
        "--cpus",
        default=DEFAULT_CPUS,
        help=f"Number of virtual CPUs (default: {DEFAULT_CPUS}).",
    )

    args = parser.parse_args()

    release = not args.debug

    print("=" * 72)
    print("PCMS BUILD / QEMU")
    print("=" * 72)
    print(f"Root:   {ROOT}")
    print(f"Target: {TARGET}")
    print(f"Mode:   {'release' if release else 'debug'}")
    print()

    check_environment()

    if args.clean:
        clean_build()

    prepare_esp()

    cargo_build(release)

    binary = locate_efi_binary(release)

    install_efi_binary(binary)

    vars_file = prepare_vars()

    if args.no_run:
        print()
        print("Build completed.")
        print(f"EFI image: {EFI_BINARY}")
        print(f"Variables: {vars_file}")
        return

    launch_qemu(
        vars_file=vars_file,
        memory=args.memory,
        cpus=args.cpus,
        headless=args.headless,
    )


if __name__ == "__main__":
    main()