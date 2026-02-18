#!/bin/bash
set -euo pipefail

version=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
tag="v${version}"
bin="kagaya"
dist="dist"

targets=(
	aarch64-apple-darwin
	x86_64-apple-darwin
	aarch64-unknown-linux-musl
	x86_64-unknown-linux-musl
)

echo "building ${bin} ${tag}"
echo

# Build the UI first
echo "building UI..."
cd ui
if ! command -v pnpm >/dev/null 2>&1; then
	echo "error: pnpm not found" >&2
	exit 1
fi
pnpm install
pnpm build
cd ..
echo

rm -rf "${dist}"
mkdir -p "${dist}"

for target in "${targets[@]}"; do
	echo "--- ${target}"

	case "${target}" in
		*-apple-*)
			cargo build --release --target "${target}"
			;;
		*-linux-*)
			cargo zigbuild --release --target "${target}"
			;;
	esac

	archive="${bin}-${tag}-${target}.tar.gz"
	tar -czf "${dist}/${archive}" -C "target/${target}/release" "${bin}"
	echo "  -> ${dist}/${archive}"
	echo
done

echo "all builds complete"
echo

ls -lh "${dist}/"
echo

read -p "create github release ${tag}? [y/N] " confirm
if [[ "${confirm}" != "y" ]]; then
	echo "skipped"
	exit 0
fi

gh release create "${tag}" \
	--title "${tag}" \
	--generate-notes \
	"${dist}"/*.tar.gz

echo
echo "released ${tag}"
echo "  https://github.com/mrgnw/kagaya/releases/tag/${tag}"
echo
echo "don't forget: cargo publish"
