"""Exercise the published-asset verification gate without network or native builds."""
import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent

class PublishedReleaseTests(unittest.TestCase):
    def test_release_waits_for_publication_before_verifying_distributed_asset(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("  verify-published:", workflow)
        job = workflow.split("  verify-published:", 1)[1]
        self.assertIn("needs: [resolve, module-release]", job)
        self.assertIn("ref: ${{ needs.resolve.outputs.tag }}", job)
        self.assertIn('scripts/verify-published-release.sh "$REPOSITORY" "$RELEASE_TAG" "$VERSION"', job)

    def verify(self, *, corrupt=False, missing_library=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts/ci").mkdir(parents=True)
            (root / "bin").mkdir()
            shutil.copy2(ROOT / "scripts/verify-published-release.sh", root / "scripts")
            shutil.copy2(ROOT / "scripts/ci/verify-linux-module-runtime-dependencies.sh", root / "scripts/ci")
            archive = "tinycomputer-1.2.3-ubuntu-24.04-x86_64.tar.gz"
            with tarfile.open(root / archive, "w:gz") as package:
                member = tarfile.TarInfo("./libtinycomputer.so")
                member.size = len(b"fixture")
                package.addfile(member, io.BytesIO(b"fixture"))
            digest = hashlib.sha256((root / archive).read_bytes()).hexdigest()
            (root / "checksum.toml").write_text(f'[sha256]\n"{archive}" = "{"0" * 64 if corrupt else digest}"\n')
            commands = {
                "gh": '#!/usr/bin/env python3\nimport pathlib,shutil,sys\na=sys.argv\n'
                      'dest=pathlib.Path(a[a.index("--dir")+1])\n'
                      'for i,v in enumerate(a):\n'
                      ' if v == "--pattern": shutil.copy2(a[i+1],dest)\n',
                "ldd": '#!/usr/bin/env python3\nfrom pathlib import Path\n'
                       'with Path("calls").open("a") as f: f.write("ldd\\n")\n'
                       f'print({"libfixture.so => not found" if missing_library else "libfixture.so => /lib/libfixture.so"!r})\n',
                "cargo": '#!/usr/bin/env python3\nimport json,sys\nfrom pathlib import Path\n'
                         'with Path("calls").open("a") as f: f.write(json.dumps(sys.argv[1:])+"\\n")\n',
            }
            for name, content in commands.items():
                executable = root / "bin" / name
                executable.write_text(content)
                executable.chmod(0o755)
            result = subprocess.run(["bash", "scripts/verify-published-release.sh",
                "tinyhumansai/tinycomputer", "v1.2.3", "1.2.3"], cwd=root,
                env=dict(os.environ, PATH=str(root / "bin") + os.pathsep + os.environ["PATH"]),
                text=True, capture_output=True)
            calls = (root / "calls").read_text() if (root / "calls").exists() else ""
            return result, calls, digest

    def test_published_archive_is_checked_before_tinybus_download_and_load(self):
        result, calls, digest = self.verify()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(calls.startswith("ldd\n"), calls)
        self.assertIn('"verify_github_release"', calls)
        self.assertIn('"https://github.com/tinyhumansai/tinycomputer/releases/tag/v1.2.3"', calls)
        self.assertIn('"tinycomputer-1.2.3-ubuntu-24.04-x86_64.tar.gz"', calls)
        self.assertIn(digest, calls)

    def test_changed_published_bytes_are_rejected_before_loading(self):
        result, calls, _ = self.verify(corrupt=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, "")

    def test_unresolved_runtime_library_prevents_loading(self):
        result, calls, _ = self.verify(missing_library=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, "ldd\n")
        self.assertIn("unresolved runtime dependencies", result.stderr)

if __name__ == "__main__": unittest.main()
