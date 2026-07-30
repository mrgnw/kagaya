#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Before/after reproduction: can `ky remove` clean up an orphaned plist?

Fully isolated from the live kagaya install — HOME and XDG_CONFIG_HOME are
redirected to /tmp/kyorphan, so config, state and ~/Library/LaunchAgents all
land there. Labels are com.kagaya.kyorphan*, which cannot collide with real
services.

An orphan is created by adding a service, then deleting its projects.toml
entry by hand — leaving the plist bootstrapped and crash-looping.

Multi-process variant checks com.kagaya.<name>.<proc>.plist are all cleaned.
"""

import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path('/tmp/kyorphan')
UID = subprocess.run(['id', '-u'], capture_output=True, text=True).stdout.strip()
BIN_OLD = ROOT / 'ky-old'
BIN_NEW = ROOT / 'ky-new'


def sh(cmd, env=None, cwd=None):
	return subprocess.run(cmd, shell=True, capture_output=True, text=True, env=env, cwd=cwd)


class Case:
	def __init__(self, name, binary, services_toml, labels):
		self.name = name
		self.binary = binary
		self.services_toml = services_toml
		self.labels = labels
		self.home = ROOT / f'home-{name}'
		self.proj = ROOT / f'proj-{name}'
		self.service = f'kyorphan{name}'

	@property
	def env(self):
		e = dict(os.environ)
		e['HOME'] = str(self.home)
		e['XDG_CONFIG_HOME'] = str(self.home / '.config')
		return e

	@property
	def agents(self):
		return self.home / 'Library' / 'LaunchAgents'

	def ky(self, *args):
		return subprocess.run(
			[str(self.binary), *args], capture_output=True, text=True, env=self.env, cwd=str(self.proj)
		)

	def plists(self):
		if not self.agents.exists():
			return []
		return sorted(p.name for p in self.agents.glob('com.kagaya.*.plist'))

	def booted(self):
		out = []
		for label in self.labels:
			full = f'com.kagaya.{self.service}{label}'
			r = sh(f'launchctl print gui/{UID}/{full}')
			out.append(f'{full}={"loaded" if r.returncode == 0 else "not-loaded"}')
		return out

	def setup(self):
		self.teardown()
		self.agents.mkdir(parents=True, exist_ok=True)
		(self.home / '.config').mkdir(parents=True, exist_ok=True)
		self.proj.mkdir(parents=True, exist_ok=True)
		(self.proj / 'services.toml').write_text(self.services_toml)
		self.ky('add', self.service)
		self.ky('start', self.service)
		time.sleep(1.5)
		# Orphan it: drop the projects.toml entry, leave the plists in place.
		projects = self.home / '.config' / 'kagaya' / 'projects.toml'
		projects.write_text('')

	def teardown(self):
		for label in self.labels:
			sh(f'launchctl bootout gui/{UID}/com.kagaya.{self.service}{label}')
		shutil.rmtree(self.home, ignore_errors=True)
		shutil.rmtree(self.proj, ignore_errors=True)


SINGLE = 'run = "sleep 9999"\n'
MULTI = '[web]\nrun = "sleep 9999"\n\n[worker]\nrun = "sleep 9999"\n'


def run_case(name, binary, services_toml, labels, extra_removes=()):
	c = Case(name, binary, services_toml, labels)
	c.setup()
	print(f'--- {name} ({binary.name}) ---')
	print(f'  before: plists={c.plists()}')
	print(f'          launchd={c.booted()}')
	r = c.ky('remove', c.service)
	print(f'  $ ky remove {c.service}')
	print(f'          exit={r.returncode} stderr={r.stderr.strip()!r}')
	time.sleep(0.5)
	print(f'  after:  plists={c.plists()}')
	print(f'          launchd={c.booted()}')
	for unknown in extra_removes:
		r = c.ky('remove', unknown)
		print(f'  $ ky remove {unknown}  (unknown name)')
		print(f'          exit={r.returncode} stderr={r.stderr.strip()!r}')
	c.teardown()
	print()


def build():
	ROOT.mkdir(parents=True, exist_ok=True)
	repo = Path(__file__).resolve().parent.parent
	for ref, dest in (('HEAD~1', BIN_OLD), ('HEAD', BIN_NEW)):
		wt = ROOT / f'src-{dest.name}'
		shutil.rmtree(wt, ignore_errors=True)
		r = sh(f'git worktree add --detach --force {wt} {ref}', cwd=str(repo))
		if r.returncode != 0:
			sys.exit(f'worktree {ref}: {r.stderr}')
		r = sh('cargo build --bin ky', cwd=str(wt))
		if r.returncode != 0:
			sys.exit(f'build {ref}: {r.stderr[-2000:]}')
		built = sh('cargo metadata --format-version 1 --no-deps', cwd=str(wt))
		import json

		target = json.loads(built.stdout)['target_directory']
		shutil.copy(Path(target) / 'debug' / 'ky', dest)
		sh(f'git worktree remove --force {wt}', cwd=str(repo))


if __name__ == '__main__':
	build()
	run_case('a', BIN_OLD, SINGLE, [''])
	run_case('b', BIN_NEW, SINGLE, [''], extra_removes=['kyorphan-nope'])
	run_case('c', BIN_NEW, MULTI, ['.web', '.worker'])
