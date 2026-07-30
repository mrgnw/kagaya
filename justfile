set shell := ["bash", "-euo", "pipefail", "-c"]

version := `grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/'`
tag := "v" + version

# Bump version: just bump patch|minor|major
bump part="patch":
	#!/bin/bash
	set -euo pipefail
	current="{{version}}"
	# 0.15.0-alpha.2 would reach $((patch + 1)) as "0-alpha.2" and die with
	# "invalid arithmetic operator". Say so instead of failing cryptically.
	case "${current}" in
		*-*) echo "refusing to bump prerelease ${current}: set the version in Cargo.toml by hand"; exit 1 ;;
	esac
	IFS='.' read -r major minor patch <<< "${current}"
	case "{{part}}" in
		patch) patch=$((patch + 1)) ;;
		minor) minor=$((minor + 1)); patch=0 ;;
		major) major=$((major + 1)); minor=0; patch=0 ;;
		*) echo "usage: just bump [patch|minor|major]"; exit 1 ;;
	esac
	next="${major}.${minor}.${patch}"
	sed -i '' "s/^version = \"${current}\"/version = \"${next}\"/" Cargo.toml
	echo "${current} -> ${next}"

# Build (debug)
build:
	cargo build --workspace

# Build (release)
build-release:
	cargo build --workspace --release

# Build the UI
build-ui:
	cd ui && pnpm install && pnpm build

# Build everything: UI + release
build-all: build-ui build-release

# Build dist archives for given targets
[private]
dist +targets: build-ui
	#!/bin/bash
	set -euo pipefail
	bin="ky"
	dist="dist"
	echo "building ${bin} {{tag}}"
	echo
	rm -rf "${dist}"
	mkdir -p "${dist}"
	for target in {{targets}}; do
		echo "--- ${target}"
		case "${target}" in
			*-apple-*)  cargo build --release --target "${target}" ;;
			*-linux-*)  ulimit -n 4096; cargo zigbuild --release --target "${target}" ;;
		esac
		archive="${bin}-{{tag}}-${target}.tar.gz"
		tar -czf "${dist}/${archive}" -C "target/${target}/release" "${bin}"
		cp "${dist}/${archive}" "${dist}/${bin}-${target}.tar.gz"
		echo "  -> ${dist}/${archive}"
		echo "  -> ${dist}/${bin}-${target}.tar.gz"
		echo
	done
	mkdir -p "${dist}/latest/completions"
	for target in {{targets}}; do
		cp "${dist}/${bin}-${target}.tar.gz" "${dist}/latest/${bin}-${target}.tar.gz"
	done
	cp install.sh "${dist}/latest/install.sh"
	cp completions/ky.bash "${dist}/latest/completions/ky.bash"
	cp completions/ky.zsh "${dist}/latest/completions/ky.zsh"
	cp completions/ky.fish "${dist}/latest/completions/ky.fish"
	echo "all builds complete"
	echo
	ls -lh "${dist}/"

# Publish dist archives as a GitHub release
[private]
gh-release confirm="yes":
	#!/bin/bash
	set -euo pipefail
	if [ "{{confirm}}" != "yes" ]; then
		read -rp "create github release {{tag}}? [y/N] " ans
		[[ "$ans" =~ ^[Yy] ]] || exit 1
	fi
	# A semver prerelease (0.15.0-alpha.3) must not become releases/latest —
	# that is what the install script and `ky self update` follow.
	prerelease=""
	case "{{version}}" in
		*-*) prerelease="--prerelease" ;;
	esac
	gh release create "{{tag}}" \
		--title "{{tag}}" \
		--generate-notes \
		${prerelease} \
		dist/*.tar.gz
	echo
	echo "released {{tag}}"
	echo "  https://github.com/mrgnw/kagaya/releases/tag/{{tag}}"
	echo
	echo "don't forget: just publish"

# Release for macOS ARM (default)
release confirm="": (dist "aarch64-apple-darwin") (gh-release confirm)

# Release for macOS (ARM + Intel)
release-macos confirm="": (dist "aarch64-apple-darwin x86_64-apple-darwin") (gh-release confirm)

# Release for Linux (ARM + x86_64)
release-linux confirm="": (dist "aarch64-unknown-linux-musl x86_64-unknown-linux-musl") (gh-release confirm)

# Release all platforms
release-all confirm="": (dist "aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl") (gh-release confirm)

# Publish to crates.io
publish:
	cargo publish -p kagaya

# Install locally (release build)
install:
	cargo install --path crates/kagaya

# Force reinstall from source (release build)
update:
	cargo install --path crates/kagaya --force

# Bump, commit, update, publish, release (skips confirmation)
ship part="patch": (bump part)
	#!/bin/bash
	set -euo pipefail
	version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
	git add Cargo.toml Cargo.lock && git commit -m "chore(release): bump version to ${version}"
	git push origin main
	just update
	just publish
	just release yes

# Install locally (debug build — fast iteration)
dev-install:
	cargo build --bin ky && cp target/debug/ky ~/.cargo/bin/ky
