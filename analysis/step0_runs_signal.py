#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Step 0 probe: does `launchctl print` expose a usable `runs` counter?

Uses its own label prefix (com.kagaya-probe.) so it is invisible to `ky`,
which only scans com.kagaya.*.

Questions from todos/restart-honours-depends-on.md:
  Q1 bootstrapped but never run  -> is `runs` absent, or 0?
  Q2 does `runs` increment on every kickstart?
  Q3 does `runs` survive bootout + bootstrap?
"""

import plistlib
import re
import subprocess
import sys
import time
from pathlib import Path

UID = subprocess.run(['id', '-u'], capture_output=True, text=True).stdout.strip()
LABEL = 'com.kagaya-probe.step0'
PLIST = Path.home() / 'Library' / 'LaunchAgents' / f'{LABEL}.plist'
TARGET = f'gui/{UID}/{LABEL}'


def launchctl(*args):
	return subprocess.run(['launchctl', *args], capture_output=True, text=True)


def write_plist(run_at_load: bool):
	body = {
		'Label': LABEL,
		'ProgramArguments': ['/bin/sh', '-c', 'sleep 3'],
		'KeepAlive': False,
		'RunAtLoad': run_at_load,
		'StandardOutPath': '/tmp/kagaya-probe-step0.log',
		'StandardErrorPath': '/tmp/kagaya-probe-step0.err.log',
	}
	PLIST.write_bytes(plistlib.dumps(body))


def probe():
	"""Return (raw_matched_lines, runs, state, pid, last_exit)."""
	out = launchctl('print', TARGET)
	if out.returncode != 0:
		return ('<print failed: %s>' % out.stderr.strip(), None, None, None, None)
	text = out.stdout
	interesting = [
		ln.strip()
		for ln in text.splitlines()
		if re.match(r'^\s*(runs|state|pid|last exit code|last exit status)\s*=', ln)
	]

	def field(name):
		m = re.search(r'^\s*%s\s*=\s*(.+?)\s*$' % re.escape(name), text, re.M)
		return m.group(1) if m else None

	return (interesting, field('runs'), field('state'), field('pid'), field('last exit code'))


def report(stage):
	lines, runs, state, pid, exit_code = probe()
	print(f'--- {stage}')
	print(f'    runs={runs!r} state={state!r} pid={pid!r} last_exit={exit_code!r}')
	print(f'    raw: {lines}')
	return runs


def cleanup():
	launchctl('bootout', TARGET)
	PLIST.unlink(missing_ok=True)


def main():
	cleanup()

	print('== Q1: bootstrapped but NEVER run (RunAtLoad=false) ==')
	write_plist(run_at_load=False)
	rc = launchctl('bootstrap', f'gui/{UID}', str(PLIST))
	if rc.returncode != 0:
		print('bootstrap failed:', rc.stderr.strip())
		return 1
	q1 = report('just bootstrapped, never kickstarted')

	print()
	print('== Q2: does `runs` increment on each kickstart? ==')
	seen = [q1]
	for i in range(1, 3):
		launchctl('kickstart', '-k', TARGET)
		time.sleep(0.4)
		seen.append(report(f'kickstart #{i} +0.4s (task should be RUNNING)'))
		time.sleep(3.2)
		seen.append(report(f'kickstart #{i} +3.6s (task should have EXITED)'))
	print(f'    runs sequence: {seen}')

	print()
	print('== Q3: does `runs` survive bootout + bootstrap? ==')
	launchctl('bootout', TARGET)
	time.sleep(0.3)
	write_plist(run_at_load=False)
	launchctl('bootstrap', f'gui/{UID}', str(PLIST))
	after = report('after bootout + bootstrap')
	print(f'    before bootout runs={seen[-1]!r} -> after bootstrap runs={after!r}')

	print()
	print('== Q4: timing — how long after kickstart does pid appear? ==')
	launchctl('kickstart', '-k', TARGET)
	t0 = time.time()
	first_pid_at = None
	for _ in range(200):
		_, _, _, pid, _ = probe()
		if pid and pid != '0':
			first_pid_at = time.time() - t0
			break
		time.sleep(0.01)
	print(f'    pid visible after {first_pid_at!r} s (None = never within 2s of polling)')

	cleanup()
	print()
	print('cleaned up:', LABEL)
	return 0


if __name__ == '__main__':
	sys.exit(main())
