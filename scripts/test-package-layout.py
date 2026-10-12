"""Verify the shipped module archive layout without compiling native code."""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent

class PackageTests(unittest.TestCase):
    def test_verifier_extracts_untrusted_archive_under_current_users_home(self):
        """Do not leave root-extracted module files in the workspace target."""
        target = ROOT / "target"; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as directory:
            root = Path(directory); bin_dir = root/"bin"; home = root/"home"
            bin_dir.mkdir(); home.mkdir()
            cargo = bin_dir/"cargo"
            cargo.write_text("#!/usr/bin/env python3\nimport json,os,sys\nfrom pathlib import Path\n"
                             "library=Path(sys.argv[-1])\n"
                             "Path(os.environ['VERIFY_ARGS']).write_text(json.dumps({"
                             "'library':str(library),'mode':library.parent.stat().st_mode & 0o777}))\n")
            cargo.chmod(0o755)
            archive = root/"module.tar.gz"
            with tarfile.open(archive, "w:gz") as package:
                for name, data in [("libtinycomputer.so", b"module"), ("modules.toml", b"module = 'fixture'\n")]:
                    member = tarfile.TarInfo(name); member.size = len(data); member.mode = 0o644
                    package.addfile(member, io.BytesIO(data))
            env = dict(os.environ, PATH=str(bin_dir)+os.pathsep+os.environ["PATH"],
                       HOME=str(home), VERIFY_ARGS=str(root/"verify-args"))
            result = subprocess.run([str(ROOT/"scripts/verify-module.sh"), "--archive", str(archive)],
                                    cwd=ROOT, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            verified = json.loads((root/"verify-args").read_text())
            library = Path(verified["library"])
            self.assertEqual(library.name, "libtinycomputer.so")
            self.assertTrue(library.is_relative_to(home), library)
            self.assertEqual(verified["mode"], 0o700)

    def test_archive_accepts_dotted_release_host_key_and_attests_library(self):
        """Package an Ubuntu matrix host key containing dots."""
        target = ROOT / "target"; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as directory:
            root = Path(directory); (root/"scripts").mkdir(); (root/"bin").mkdir()
            shutil.copy2(ROOT/"scripts/build-module.sh", root/"scripts/build-module.sh")
            for file in ["LICENSE", "README.md", "MODULE.md"]: (root/file).write_text("fixture")
            cargo = root/"bin/cargo"
            cargo.write_text(
                "#!/usr/bin/env python3\nimport json,sys\nfrom pathlib import Path\n"
                "root=Path.cwd()\n"
                "if 'metadata' in sys.argv: print(json.dumps({'target_directory':str(root/'target')}))\n"
                "else:\n"
                " target=sys.argv[sys.argv.index('--target')+1]\n"
                " output=root/'target'/target/'release'; output.mkdir(parents=True,exist_ok=True)\n"
                " (output/'libtinycomputer.so').write_bytes(b'compiled-fixture')\n"
            )
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=str(root/"bin")+os.pathsep+os.environ["PATH"])
            env.pop("CARGO_TARGET_DIR", None)
            host = "ubuntu-22.04-x86_64"
            result = subprocess.run(
                ["bash", str(root/"scripts/build-module.sh"), "--crate", "tinycomputer",
                 "--version", "1.2.3", "--target", "x86_64-unknown-linux-gnu", "--host", host],
                cwd=root, env=env, text=True, capture_output=True, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            archive = root/f"release-assets/tinycomputer-1.2.3-{host}.tar.gz"
            with tarfile.open(archive) as package:
                data = package.extractfile("./libtinycomputer.so").read()
                manifest = tomllib.loads(package.extractfile("./modules.toml").read().decode())
                self.assertEqual(data, b"compiled-fixture")
                self.assertEqual(manifest, {"libtinycomputer.so": hashlib.sha256(data).hexdigest()})

    def test_arch_package_install_disables_only_landlock_filesystem_sandbox(self):
        """Pacman receives the filesystem-only sandbox exception offline."""
        target = ROOT / "target"; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as directory:
            root = Path(directory); bin_dir = root/"bin"; bin_dir.mkdir()
            (bin_dir/"bash").symlink_to(shutil.which("bash"))
            pacman = bin_dir/"pacman"
            pacman.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$PACMAN_ARGS\"\n")
            pacman.chmod(0o755)
            env = {"PATH": str(bin_dir), "PACMAN_ARGS": str(root/"pacman-args")}
            result = subprocess.run([str(ROOT/"scripts/install-distro-packages.sh")],
                                    cwd=ROOT, env=env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            args = (root/"pacman-args").read_text().splitlines()
            self.assertEqual(args, ["-Syu", "--disable-sandbox-filesystem", "--noconfirm",
                                    "base-devel", "cmake", "curl", "git", "gzip", "jq",
                                    "openssl", "perl", "alsa-lib", "xdotool"])
            self.assertNotIn("--disable-sandbox", args)

    def test_archive_attests_host_library_bytes_from_a_symlinked_checkout(self):
        """Package the actual build script and verify the shipped bytes and attestation."""
        target = ROOT / "target"; target.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=target) as directory:
            root = Path(directory); (root/"scripts").mkdir(); (root/"bin").mkdir()
            shutil.copy2(ROOT/"scripts/build-module.sh", root/"scripts/build-module.sh")
            for file in ["LICENSE","README.md","MODULE.md"]: (root/file).write_text("fixture")
            cargo = root/"bin/cargo"
            cargo.write_text("#!/usr/bin/env python3\nimport json,sys\nfrom pathlib import Path\n"
                "root=Path.cwd()\n"
                "if 'metadata' in sys.argv: print(json.dumps({'target_directory':str(root/'target')}))\n"
                "else:\n"
                " target=sys.argv[sys.argv.index('--target')+1]\n"
                " output=root/'target'/target/'release'; output.mkdir(parents=True,exist_ok=True)\n"
                " (output/'libtinycomputer.so').write_bytes(b'compiled-fixture')\n")
            cargo.chmod(0o755)
            env = dict(os.environ, PATH=str(root/"bin")+os.pathsep+os.environ["PATH"])
            env.pop("CARGO_TARGET_DIR", None)
            alias = root/"checkout-link"; alias.symlink_to(root, target_is_directory=True)
            result = subprocess.run(["bash",str(alias/"scripts/build-module.sh"),
                "--crate","tinycomputer","--version","1.2.3-beta.1",
                "--target","x86_64-unknown-linux-gnu","--host","linux-fixture"],
                cwd=root,env=env,text=True,capture_output=True)
            self.assertEqual(result.returncode,0,result.stderr)
            archive=root/"release-assets/tinycomputer-1.2.3-beta.1-linux-fixture.tar.gz"
            with tarfile.open(archive) as package:
                data=package.extractfile("./libtinycomputer.so").read()
                manifest=tomllib.loads(package.extractfile("./modules.toml").read().decode())
                self.assertEqual(data,b"compiled-fixture")
                self.assertEqual(manifest,{"libtinycomputer.so":hashlib.sha256(data).hexdigest()})
                for name in ["./LICENSE","./README.md","./MODULE.md"]:
                    self.assertIsNotNone(package.extractfile(name))

if __name__ == "__main__": unittest.main()
