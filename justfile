set shell := ["bash", "-euo", "pipefail", "-c"]

version := `grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/'`
tag := "v" + version

# Bump version: just bump patch|minor|major
bump part="patch":
	#!/bin/bash
	set -euo pipefail
	current="{{version}}"
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
	bin="kagaya"
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
		echo "  -> ${dist}/${archive}"
		echo
	done
	echo "all builds complete"
	echo
	ls -lh "${dist}/"

# Publish dist archives as a GitHub release
[private]
gh-release:
	#!/bin/bash
	set -euo pipefail
	echo
	read -p "create github release {{tag}}? [y/N] " confirm
	if [[ "${confirm}" != "y" ]]; then
		echo "skipped"
		exit 0
	fi
	gh release create "{{tag}}" \
		--title "{{tag}}" \
		--generate-notes \
		dist/*.tar.gz
	echo
	echo "released {{tag}}"
	echo "  https://github.com/mrgnw/kagaya/releases/tag/{{tag}}"
	echo
	echo "don't forget: just publish"

# Release for macOS ARM (default)
release: (dist "aarch64-apple-darwin") gh-release

# Release for macOS (ARM + Intel)
release-macos: (dist "aarch64-apple-darwin x86_64-apple-darwin") gh-release

# Release for Linux (ARM + x86_64)
release-linux: (dist "aarch64-unknown-linux-musl x86_64-unknown-linux-musl") gh-release

# Release all platforms
release-all: (dist "aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl") gh-release

# Publish to crates.io
publish:
	cargo publish -p kagaya

# Install locally (release build)
install:
	cargo install --path crates/kagaya
	ln -sf ~/.cargo/bin/kagaya ~/.cargo/bin/ky

# Install locally (debug build — fast iteration)
dev-install:
	cargo build --bin kagaya && cp target/debug/kagaya ~/.cargo/bin/kagaya
	ln -sf ~/.cargo/bin/kagaya ~/.cargo/bin/ky
