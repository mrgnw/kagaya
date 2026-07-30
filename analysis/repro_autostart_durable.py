#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Before/after reproduction: does `ky autostart <svc> off` survive `ky restart`?

Fully isolated from the live kagaya install — HOME *and* XDG_CONFIG_HOME are
redirected per variant, so config, state and ~/Library/LaunchAgents all land
under /tmp/kyautodur. Labels are com.kagaya.kyautodur<variant>.*, which cannot
collide with real services.

Expected:
	buggy — RunAtLoad flips back to true on restart (config still says true)
	fixed — RunAtLoad stays false, and projects.toml says autostart = false
"""

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path('/tmp/kyautodur')
UID = subprocess.run(['id', '-u'], capture_output=True, text=True).stdout.strip()


class Variant:
	def __init__(self, name, binary):
		self.name = name
		self.binary = ROOT / binary
		self.home = ROOT / f'home-{name}'
		self.proj = ROOT / f'proj-{name}'
		self.service = f'kyautodur{name}'

	@property
	def env(self):
		e = dict(os.environ)
		e['HOME'] = str(self.home)
		e['XDG_CONFIG_HOME'] = str(self.home / '.config')
		return e

	@property
	def projects_toml(self):
		return self.home / '.config' / 'kagaya' / 'projects.toml'

	def ky(self, *args):
		return subprocess.run(
			[str(self.binary), *args], capture_output=True, text=True, env=self.env, cwd=str(self.proj)
		)

	def setup(self):
		self.teardown()
		(self.home / 'Library' / 'LaunchAgents').mkdir(parents=True, exist_ok=True)
		self.proj.mkdir(parents=True, exist_ok=True)
		(self.proj / 'services.toml').write_text('[web]\nrun = "sleep 300"\n')
		self.projects_toml.parent.mkdir(parents=True, exist_ok=True)
		self.projects_toml.write_text(
			f'# isolated repro\n\n[{self.service}]\ndir = "{self.proj}"\nautostart = true\n'
		)
		self.ky('start', self.service)
		time.sleep(2)

	def teardown(self):
		for p in (self.home / 'Library' / 'LaunchAgents').glob('com.kagaya.*.plist'):
			subprocess.run(['launchctl', 'bootout', f'gui/{UID}/{p.stem}'], capture_output=True)
		for label in (f'com.kagaya.{self.service}', f'com.kagaya.{self.service}.web'):
			subprocess.run(['launchctl', 'bootout', f'gui/{UID}/{label}'], capture_output=True)
		for d in (self.home, self.proj):
			shutil.rmtree(d, ignore_errors=True)

	def plists(self):
		return sorted((self.home / 'Library' / 'LaunchAgents').glob(f'com.kagaya.{self.service}*.plist'))

	def run_at_load(self):
		vals = []
		for p in self.plists():
			out = subprocess.run(
				['plutil', '-extract', 'RunAtLoad', 'raw', str(p)], capture_output=True, text=True
			)
			vals.append(f'{p.name}={out.stdout.strip() or out.stderr.strip()}')
		return ', '.join(vals) or '(no plist)'

	def config_autostart(self):
		for line in self.projects_toml.read_text().splitlines():
			if line.strip().startswith('autostart'):
				return line.strip()
		return '(no autostart key)'


def run_variant(name, binary):
	v = Variant(name, binary)
	print(f'== {name} ({binary}) ==')
	v.setup()
	print(f'  after start          plist: {v.run_at_load()}   config: {v.config_autostart()}')

	res = v.ky('autostart', v.service, 'off')
	out = (res.stdout + res.stderr).strip()
	print(f'  ky autostart off     rc={res.returncode} {out!r}')
	print(f'                       plist: {v.run_at_load()}   config: {v.config_autostart()}')

	v.ky('restart', v.service)
	time.sleep(2)
	after = v.run_at_load()
	cfg = v.config_autostart()
	print(f'  after ky restart     plist: {after}   config: {cfg}')

	verdict = 'DURABLE' if 'true' not in after else 'REVERTED'
	print(f'  => {verdict}\n')
	v.ky('stop', v.service)
	time.sleep(1)
	v.teardown()
	return verdict


def main():
	missing = [b for b in ('ky-buggy', 'ky-fixed') if not (ROOT / b).exists()]
	if missing:
		print(f'build {" and ".join(str(ROOT / m) for m in missing)} first')
		return 1
	before = run_variant('buggy', 'ky-buggy')
	after = run_variant('fixed', 'ky-fixed')
	print(f'summary: buggy={before}  fixed={after}')
	return 0 if (before, after) == ('REVERTED', 'DURABLE') else 1


if __name__ == '__main__':
	sys.exit(main())
