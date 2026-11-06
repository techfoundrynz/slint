# SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0
"""Validate an MCU package's headers, licenses, and static-library architecture."""

import argparse
from pathlib import Path
import struct
import tarfile

TARGET_MACHINES = {"xtensa-esp32s3-none-elf": 94, "riscv32imafc-esp-espidf": 243}


def validate_library(data, machine):
    if not data.startswith(b"!<arch>\n"):
        raise ValueError("The runtime is not a static archive")
    position = 8
    objects = 0
    while position < len(data):
        header = data[position : position + 60]
        if len(header) != 60 or header[58:60] != b"`\n":
            raise ValueError("Invalid static archive member")
        size = int(header[48:58])
        member = data[position + 60 : position + 60 + size]
        if len(member) != size:
            raise ValueError("Truncated static archive member")
        if member.startswith(b"\x7fELF"):
            if len(member) < 52 or member[4:6] != b"\x01\x01":
                raise ValueError(
                    "The runtime requires 32-bit little-endian ELF objects"
                )
            if struct.unpack_from("<H", member, 18)[0] != machine:
                raise ValueError(
                    "The runtime contains an object for another architecture"
                )
            if machine == 243 and struct.unpack_from("<I", member, 36)[0] & 6 != 2:
                raise ValueError(
                    "P4 objects require the single-precision floating-point ABI"
                )
            objects += 1
        position += 60 + size + (size % 2)
    if position != len(data):
        raise ValueError("Truncated static archive padding")
    if not objects:
        raise ValueError("The runtime contains no ELF objects")
    return objects


def validate_package(archive, target):
    if not archive.name.endswith(f"-{target}.tar.gz"):
        raise ValueError("The package filename does not match the Rust target")
    with tarfile.open(archive) as package:
        members = {item.name: item for item in package.getmembers() if item.isfile()}
        required = [
            "include/slint/slint.h",
            "include/slint/private/slint_image_internal.h",
            "lib/cmake/Slint/SlintConfig.cmake",
            "licenses/LICENSE.md",
            "licenses/THIRDPARTY.md",
        ]
        for suffix in required:
            if not any(name.endswith("/" + suffix) for name in members):
                raise ValueError(f"Missing package file: {suffix}")
        libraries = [
            item
            for name, item in members.items()
            if name.endswith("/lib/libslint_cpp.a")
        ]
        if len(libraries) != 1:
            raise ValueError("The package must contain one libslint_cpp.a")
        objects = validate_library(
            package.extractfile(libraries[0]).read(), TARGET_MACHINES[target]
        )
    print(
        f"Validated {archive.name}: {objects} ELF objects, headers, CMake configuration, and licenses"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("--target", choices=TARGET_MACHINES, required=True)
    args = parser.parse_args()
    validate_package(args.archive, args.target)


if __name__ == "__main__":
    main()
