"""The complete maintenance path must be wired, pinned and credential-free on PRs."""
from pathlib import Path
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent


class MaintenancePolicyTests(unittest.TestCase):
    def test_one_pinned_release_pipeline(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        self.assertRegex(workflow, r'cargo-maintenance.yml@[0-9a-f]{40}')
        self.assertNotIn('policy-revision:', workflow)
        self.assertIn('schedule:', workflow)
        self.assertIn('workflow_dispatch:', workflow)
        for path in (ROOT / '.github/workflows').glob('*.yml'):
            self.assertNotRegex(path.read_text(), r'cargo-auto-version-bump.yml|cargo-crate-ci.yml|--auto --rebase')

    def test_daily_scoped_maintenance(self):
        config = (ROOT / '.github/dependabot.yml').read_text()
        self.assertEqual(config.count('interval: daily'), 2)
        self.assertNotIn('include: scope', config)
        self.assertIn('prefix: "build(deps)"', config)
        self.assertIn('security-updates:', config)
        self.assertIn('dependency-type: all', config)
        merge = (ROOT / '.github/workflows/dependabot-merge.yml').read_text()
        self.assertIn('merge-tested-dependabot@', merge)
        self.assertIn('actions/create-github-app-token@', merge)

    def test_compiler_and_formatter_pins_match_hooks(self):
        package = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']
        compiler = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
        self.assertEqual(compiler, package['rust-version'])
        self.assertRegex((ROOT / 'rustfmt-toolchain').read_text().strip(), r'^nightly-\d{4}-\d{2}-\d{2}$')
        hook = (ROOT / '.githooks/pre-commit').read_text()
        self.assertIn('scripts/fmt.sh --check', hook)
        self.assertNotIn('+nightly ', hook)

    def test_windows_checkout_preserves_shell_and_toolchain_lf(self):
        self.assertIn('* text=auto eol=lf', (ROOT / '.gitattributes').read_text())

    def test_package_contains_integration_tests_and_licenses(self):
        package = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']
        self.assertIn('tests/**', package['include'])
        self.assertIn('LICENSE-MIT', package['include'])
        self.assertIn('LICENSE-APACHE', package['include'])
        self.assertTrue(any((ROOT / 'tests').glob('*.rs')))


if __name__ == '__main__':
    unittest.main()
