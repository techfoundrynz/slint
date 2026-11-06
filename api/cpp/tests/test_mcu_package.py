# SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

import importlib.util
import io
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "mcu_package",
    Path(__file__).resolve().parents[3] / "scripts/validate_mcu_package.py",
)
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


def library(machine, flags=3):
    elf = bytearray(52)
    elf[:6] = b"\x7fELF\x01\x01"
    struct.pack_into("<H", elf, 18, machine)
    struct.pack_into("<I", elf, 36, flags)
    header = (
        f"{'runtime.o/':<16}{0:<12}{0:<6}{0:<6}{'100644':<8}{len(elf):<10}`\n".encode()
    )
    return b"!<arch>\n" + header + elf


class PackageTests(unittest.TestCase):
    def test_target_architectures(self):
        for target, machine in validator.TARGET_MACHINES.items():
            with self.subTest(target=target):
                self.assertEqual(
                    validator.validate_library(library(machine), machine), 1
                )

    def test_wrong_architecture(self):
        with self.assertRaisesRegex(ValueError, "another architecture"):
            validator.validate_library(library(94), 243)

    def test_wrong_float_abi(self):
        for flags in [0, 4, 6]:
            with (
                self.subTest(flags=flags),
                self.assertRaisesRegex(ValueError, "floating-point ABI"),
            ):
                validator.validate_library(library(243, flags), 243)

    def test_invalid_archive(self):
        for data in [b"", b"!<arch>\n", library(94)[:-1], library(94) + b"x"]:
            with self.subTest(data=data[:8]), self.assertRaises(ValueError):
                validator.validate_library(data, 94)

    def test_package_contents(self):
        for target, machine in validator.TARGET_MACHINES.items():
            with (
                self.subTest(target=target),
                tempfile.TemporaryDirectory() as directory,
            ):
                archive = Path(directory) / f"Slint-cpp-1.19.0-{target}.tar.gz"
                required = {
                    "include/slint/slint.h": b"header",
                    "include/slint/private/slint_image_internal.h": b"generated header",
                    "lib/cmake/Slint/SlintConfig.cmake": b"cmake",
                    "licenses/LICENSE.md": b"license",
                    "licenses/THIRDPARTY.md": b"notices",
                    "lib/libslint_cpp.a": library(machine),
                }
                for complete in [True, False]:
                    with tarfile.open(archive, "w:gz") as output:
                        for name, data in required.items():
                            if not complete and name == "licenses/THIRDPARTY.md":
                                continue
                            info = tarfile.TarInfo("Slint-cpp/" + name)
                            info.size = len(data)
                            output.addfile(info, io.BytesIO(data))
                    if complete:
                        validator.validate_package(archive, target)
                    else:
                        with self.assertRaisesRegex(ValueError, "Missing package file"):
                            validator.validate_package(archive, target)
                with self.assertRaisesRegex(ValueError, "filename"):
                    validator.validate_package(
                        archive.with_name("wrong.tar.gz"), target
                    )


if __name__ == "__main__":
    unittest.main()
