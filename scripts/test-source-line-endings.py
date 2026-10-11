"""Keep embedded skill bytes stable in Windows Git checkouts."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class SourceLineEndingTests(unittest.TestCase):
    def test_windows_checkout_preserves_embedded_skill_line_endings(self):
        """core.autocrlf must not rewrite the embedded Markdown to CRLF."""
        root = Path(__file__).resolve().parent.parent
        asset = Path("crates/tinycomputer-skills/skills/tinycomputer/SKILL.md")
        with tempfile.TemporaryDirectory() as directory:
            checkout = Path(directory)
            subprocess.run(["git", "init", "--quiet", directory], check=True)
            subprocess.run(["git", "config", "core.autocrlf", "true"],
                           cwd=checkout, check=True)
            attributes = root / ".gitattributes"
            if attributes.exists():
                shutil.copyfile(attributes, checkout / ".gitattributes")
            (checkout / asset).parent.mkdir(parents=True)
            shutil.copyfile(root / asset, checkout / asset)
            subprocess.run(["git", "add", "--", asset.as_posix()],
                           cwd=checkout, check=True, capture_output=True)
            subprocess.run(["git", "checkout-index", "--prefix=output/", "--",
                            asset.as_posix()], cwd=checkout, check=True)
            contents = (checkout / "output" / asset).read_bytes()
            self.assertFalse(b"\r\n" in contents, "embedded skill was checked out with CRLF")
            self.assertEqual(contents, (root / asset).read_bytes())


if __name__ == "__main__":
    unittest.main()
