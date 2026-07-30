#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Before/after reproduction: does `ky restart` honour depends_on?

Fully isolated from the live kagaya install — HOME is redirected per variant, so
config, state and ~/Library/LaunchAgents all land under /tmp/kyrepro. Labels are
com.kagaya.kyrepro<variant>.*, which cannot collide with real services.

The service is:
	[build]  type = "task", sleeps then stamps build-done
	[web]    depends_on = "build", stamps web-start then sleeps

web-start BEFORE build-done  => dependent started while the task was mid-flight
web-start AFTER  build-done  => the barrier held
"""

import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path('/tmp/kyrepro')
BUILD_SECS = 4
UID = subprocess.run(['id', '-u'], capture_output=True, text=True).stdout.strip()


def sh(cmd, env=None, cwd=None):
	return subprocess.run(cmd, shell=True, capture_output=True, text=True, env=env, cwd=cwd)


class Variant:
	def __init__(self, name, binary, autostart):
		self.name = name
		self.binary = ROOT / binary
		self.autostart = autostart
		self.home = ROOT / f'home-{name}'
		self.proj = ROOT / f'proj-{name}'
		self.marks = ROOT / f'marks-{name}'
		self.service = f'kyrepro{name}'

	@property
	def env(self):
		import os

		e = dict(os.environ)
		e['HOME'] = str(self.home)
		return e

	def ky(self, *args):
		return subprocess.run(
			[str(self.binary), *args], capture_output=True, text=True, env=self.env, cwd=str(self.proj)
		)

	def setup(self):
		self.teardown()
		(self.home / 'Library' / 'LaunchAgents').mkdir(parents=True, exist_ok=True)
		self.proj.mkdir(parents=True, exist_ok=True)
		self.marks.mkdir(parents=True, exist_ok=True)
		(self.proj / 'services.toml').write_text(
			'[build]\n'
			f'run = "sleep {BUILD_SECS}; date +%s.%N > {self.marks}/build-done"\n'
			'type = "task"\n'
			'\n'
			'[web]\n'
			f'run = "date +%s.%N > {self.marks}/web-start; sleep 300"\n'
			'depends_on = "build"\n'
		)
		# Register by hand rather than via `ky add`, so autostart is explicit.
		cfg = self.home / '.config' / 'kagaya'
		cfg.mkdir(parents=True, exist_ok=True)
		(cfg / 'projects.toml').write_text(
			f'[{self.service}]\ndir = "{self.proj}"\nautostart = {str(self.autostart).lower()}\n'
		)
		# Warm-up: get the plists written and bootstrapped once, then stop
		# everything. A first `ky start` on fresh plists lets launchd's
		# RunAtLoad race ky; from here on the plists are unchanged, so
		# `sync_service` is a no-op and ky alone drives the ordering.
		self.ky('start', self.service)
		time.sleep(BUILD_SECS + 3)
		self.ky('stop', self.service)
		time.sleep(1.5)
		return True

	def diagnose(self):
		print('      --- diagnostics ---')
		print(f'      marks: {sorted(p.name for p in self.marks.glob("*"))}')
		for label in (f'com.kagaya.{self.service}.build', f'com.kagaya.{self.service}.web'):
			out = sh(f'launchctl print gui/{UID}/{label}')
			fields = [
				ln.strip()
				for ln in out.stdout.splitlines()
				if ln.strip().startswith(('state =', 'runs =', 'pid =', 'last exit code ='))
			]
			print(f'      {label}: {fields or out.stderr.strip()[:80]}')
		for log in sorted(self.home.rglob('*.log')):
			body = log.read_text().strip()
			if body:
				print(f'      {log.name}: {body.splitlines()[-3:]}')

	def teardown(self):
		for label in (
			f'com.kagaya.{self.service}.build',
			f'com.kagaya.{self.service}.web',
			f'com.kagaya.{self.service}',
		):
			subprocess.run(['launchctl', 'bootout', f'gui/{UID}/{label}'], capture_output=True, text=True)
		for d in (self.home, self.proj, self.marks):
			shutil.rmtree(d, ignore_errors=True)

	def clear_marks(self):
		for f in self.marks.glob('*'):
			f.unlink()

	def stamp(self, which):
		p = self.marks / which
		if not p.exists():
			return None
		raw = p.read_text().strip()
		try:
			return float(raw)
		except ValueError:
			return None

	def observe(self, action_label, run_action):
		"""Run an action, then wait out the task and compare the two stamps."""
		self.clear_marks()
		t0 = time.time()
		res = run_action()
		elapsed = time.time() - t0
		# Let the task finish and the dependent land, whatever the ordering.
		time.sleep(BUILD_SECS + 4)
		build_done = self.stamp('build-done')
		web_start = self.stamp('web-start')

		print(f'  [{self.name}] {action_label}: returned after {elapsed:.1f}s, rc={res.returncode}')
		for line in (res.stdout or '').strip().splitlines():
			print(f'      out| {line}')
		for line in (res.stderr or '').strip().splitlines():
			print(f'      err| {line}')

		if build_done is None and web_start is not None:
			# The task never completed a run, yet the dependent was started.
			print('      => build never ran, web STARTED ANYWAY  BUG: dependent started early')
			self.diagnose()
			return 'web-without-build'
		if build_done is None and web_start is None:
			print('      => build never ran, web correctly not started  BARRIER HELD')
			self.diagnose()
			return 'both-skipped'
		if web_start is None:
			print('      => build ran, web not started')
			self.diagnose()
			return 'web-skipped'
		delta = web_start - build_done
		verdict = 'BARRIER HELD' if delta > 0 else 'BUG: dependent started early'
		print(f'      => web_start - build_done = {delta:+.2f}s  {verdict}')
		return delta


def run_variant(name, binary, autostart):
	v = Variant(name, binary, autostart)
	print(f'== {name} ({binary}, autostart={autostart}) ==')
	if not v.setup():
		v.teardown()
		return {}

	results = {}
	results['start'] = v.observe('ky start', lambda: v.ky('start', v.service))
	# Settle: make sure nothing is mid-run before the restart measurement.
	time.sleep(1.5)
	results['restart'] = v.observe('ky restart', lambda: v.ky('restart', v.service))
	v.teardown()
	print()
	return results


def main():
	if not (ROOT / 'ky-fixed').exists() or not (ROOT / 'ky-buggy').exists():
		print('build /tmp/kyrepro/ky-fixed and ky-buggy first')
		return 1

	rows = []
	for autostart in (True, False):
		tag = 'on' if autostart else 'off'
		before = run_variant(f'buggy{tag}', 'ky-buggy', autostart)
		after = run_variant(f'fixed{tag}', 'ky-fixed', autostart)
		rows.append((f'autostart={tag}', before, after))

	print('== summary (web_start - build_done, seconds) ==')
	print(f'{"":16} {"":8} {"ky start":>14} {"ky restart":>14}')

	labels = {
		'web-without-build': 'BUG web-no-build',
		'both-skipped': 'ok both skipped',
		'web-skipped': 'web skipped',
		None: 'n/a',
	}

	def fmt(x):
		if isinstance(x, str) or x is None:
			return labels.get(x, str(x))
		return f'{x:+.2f}'

	for tag, before, after in rows:
		for label, r in (('before', before), ('after', after)):
			print(f'{tag:16} {label:8} {fmt(r.get("start")):>18} {fmt(r.get("restart")):>18}')
	print()
	print('  +N.NN             barrier held: web started N.NN s after build finished')
	print('  -N.NN             BUG: web started N.NN s BEFORE build finished')
	print('  BUG web-no-build  BUG: build never ran at all, web started regardless')
	print('  ok both skipped   build never ran, web correctly withheld')
	return 0


if __name__ == '__main__':
	sys.exit(main())
