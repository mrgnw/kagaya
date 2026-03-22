#!/bin/sh
set -eu

repo="mrgnw/kagaya"
bin="ky"
install_dir="${INSTALL_DIR:-${HOME}/.local/bin}"
install_base_url_default="https://ky.xcc.es"
base_url="${INSTALL_BASE_URL:-${install_base_url_default}}"

detect_target() {
	os=$(uname -s)
	arch=$(uname -m)

	case "${os}" in
		Darwin) os_part="apple-darwin" ;;
		Linux)  os_part="unknown-linux-musl" ;;
		*)
			echo "unsupported OS: ${os}" >&2
			exit 1
			;;
	esac

	case "${arch}" in
		x86_64|amd64)  arch_part="x86_64" ;;
		arm64|aarch64) arch_part="aarch64" ;;
		*)
			echo "unsupported architecture: ${arch}" >&2
			exit 1
			;;
	esac

	echo "${arch_part}-${os_part}"
}

download() {
	url="$1"
	dest="$2"
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL -o "${dest}" "${url}"
	elif command -v wget >/dev/null 2>&1; then
		wget -qO "${dest}" "${url}"
	else
		echo "curl or wget required" >&2
		return 1
	fi
}

target=$(detect_target)
archive="${bin}-${target}.tar.gz"
hosted_url="${base_url%/}/releases/latest/${archive}"
github_url="https://github.com/${repo}/releases/latest/download/${archive}"

echo "installing ${bin} (${target})"

tmpdir=$(mktemp -d)
trap 'rm -rf "${tmpdir}"' EXIT

if ! download "${hosted_url}" "${tmpdir}/${archive}"; then
	echo "hosted binary unavailable, falling back to GitHub release"
	download "${github_url}" "${tmpdir}/${archive}"
fi
tar -xzf "${tmpdir}/${archive}" -C "${tmpdir}"

mkdir -p "${install_dir}"
mv "${tmpdir}/${bin}" "${install_dir}/${bin}"
chmod +x "${install_dir}/${bin}"

echo "installed ${install_dir}/${bin}"

# --- install shell completions ---

completion_dir="${HOME}/.local/share/kagaya/completions"
mkdir -p "${completion_dir}"

for shell in bash zsh fish; do
	hosted_completion_url="${base_url%/}/releases/latest/completions/ky.${shell}"
	github_completion_url="https://raw.githubusercontent.com/${repo}/latest/completions/ky.${shell}"
	download "${hosted_completion_url}" "${completion_dir}/ky.${shell}" 2>/dev/null || \
		download "${github_completion_url}" "${completion_dir}/ky.${shell}" 2>/dev/null || true
done

if [ -f "${completion_dir}/ky.bash" ]; then
	echo
	echo "shell completions installed to ${completion_dir}"
	echo
	echo "enable tab completion:"
	echo
	echo "  bash:"
	echo "    echo 'source ${completion_dir}/ky.bash' >> ~/.bashrc"
	echo
	echo "  zsh:"
	echo "    echo 'fpath=(${completion_dir} \$fpath)' >> ~/.zshrc"
	echo "    echo 'autoload -Uz compinit && compinit' >> ~/.zshrc"
	echo
	echo "  fish:"
	echo "    ln -s ${completion_dir}/ky.fish ~/.config/fish/completions/"
fi

# --- PATH hint ---

if ! echo "${PATH}" | tr ':' '\n' | grep -qx "${install_dir}"; then
	echo
	echo "add to your PATH:"
	echo "  export PATH=\"${install_dir}:\${PATH}\""
fi
