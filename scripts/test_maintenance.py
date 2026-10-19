"""The complete maintenance path must be wired, pinned and credential-free on PRs."""
from pathlib import Path
import os
import subprocess
import sys
import textwrap
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent


class MaintenancePolicyTests(unittest.TestCase):
    def test_completion_gate_rejects_every_incomplete_pipeline_result(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        self.assertIn('  test:\n', workflow)
        gate = workflow.split('  test:\n', 1)[1]
        self.assertIn('if: always()', gate)
        self.assertIn('needs: [prepare, ci]', gate)
        self.assertIn('PREPARE_RESULT: ${{ needs.prepare.result }}', gate)
        self.assertIn('CI_RESULT: ${{ needs.ci.result }}', gate)
        script = textwrap.dedent(gate.split("python3 - <<'PYTHON'\n", 1)[1].split('          PYTHON', 1)[0])
        for prepare, pipeline in [('success', 'success'), ('failure', 'success'),
                                  ('success', 'failure'), ('success', 'cancelled'),
                                  ('success', 'skipped')]:
            with self.subTest(prepare=prepare, pipeline=pipeline):
                result = subprocess.run([sys.executable, '-c', script], capture_output=True,
                                        env={**os.environ, 'PREPARE_RESULT': prepare, 'CI_RESULT': pipeline})
                self.assertEqual(result.returncode == 0, prepare == pipeline == 'success')

    def test_dependency_merger_has_a_separate_identity_and_requires_success(self):
        merge = (ROOT / '.github/workflows/dependabot-merge.yml').read_text()
        self.assertIn("if: github.event.workflow_run.conclusion == 'success'", merge)
        self.assertIn('secrets.RELEASE_MERGER_APP_ID', merge)
        self.assertIn('secrets.RELEASE_MERGER_APP_KEY', merge)
        self.assertNotIn('RELEASE_WRITER', merge)
        action = merge.split('uses: legra-ai/github-actions/.github/actions/merge-tested-dependabot@', 1)[1]
        self.assertIn('app-id: ${{ secrets.RELEASE_MERGER_APP_ID }}', action)
        self.assertIn('token: ${{ steps.merger.outputs.token }}', action)

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
        if (ROOT / 'build.rs').exists():
            self.assertIn('build.rs', package['include'])
        self.assertIn('tests/**', package['include'])
        self.assertIn('LICENSE-MIT', package['include'])
        self.assertIn('LICENSE-APACHE', package['include'])
        self.assertTrue(any((ROOT / 'tests').glob('*.rs')))


class CommitMessageTests(unittest.TestCase):
    def test_breaking_scope_and_headline_validation(self):
        import os
        import subprocess
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cog = root / 'cog'
            # Isolate the scope/headline gate; cocogitto has its own type parser.
            cog.write_text('#!/bin/sh\nexit 0\n', newline='\n')
            cog.chmod(0o755)
            message = root / 'message'
            env = {**os.environ, 'PATH': str(root) + os.pathsep + os.environ['PATH']}
            for headline, body, expected in [
                ('build(deps): update serde', '', 0),
                ('ci(deps-dev): update action', '', 0),
                ('feat(api)!: change contract', '', 0),
                ('fix(api): repair', '\nBREAKING CHANGE: new wire shape\n', 0),
                ('fix: missing scope', '', 1),
                ('oops(api): invalid type', '', 1),
                ('bad headline', '\nfix(api): forged body\n', 1),
                ('fix: missing scope', '\nfix(api): forged body\n', 1),
                ('fix(api): ', '', 1),
            ]:
                with self.subTest(headline=headline):
                    message.write_text(headline + '\n' + body, newline='\n')
                    # Windows also provides a WSL bash; use Git's own shell.
                    shell = (Path(subprocess.check_output(['git', '--exec-path'], text=True).strip()).parents[2]
                             / 'bin/bash.exe') if os.name == 'nt' else 'bash'
                    result = subprocess.run([str(shell), str(ROOT / '.githooks/commit-msg'), str(message)],
                                            env=env, capture_output=True, text=True)
                    self.assertEqual(result.returncode, expected, result.stdout + result.stderr)


class BuildOwnershipTests(unittest.TestCase):
    def test_hook_dependency_excludes_recursive_installers(self):
        manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
        requirement = manifest['build-dependencies']['rhusky']
        self.assertRegex(requirement, r'^2\.[0-9]+\.[0-9]+$')
        self.assertGreaterEqual(tuple(map(int, requirement.split('.'))), (2, 0, 1))

    def test_hook_ownership_is_an_explicit_local_argument(self):
        build = (ROOT / 'build.rs').read_text()
        self.assertIn('install_from_build_script(env!("CARGO_MANIFEST_DIR"))', build)
        config = ROOT / '.cargo/config.toml'
        if config.exists():
            self.assertNotIn('RHUSKY_REPOSITORY_ROOT', config.read_text())


if __name__ == '__main__':
    unittest.main()
