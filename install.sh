#!/bin/sh
set -eu

repo="mrgnw/kagaya"
bin="kagaya"
install_dir="${INSTALL_DIR:-${HOME}/.local/bin}"

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

latest_tag() {
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL "https://api.github.com/repos/${repo}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
	elif command -v wget >/dev/null 2>&1; then
		wget -qO- "https://api.github.com/repos/${repo}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
	else
		echo "curl or wget required" >&2
		exit 1
	fi
}

download() {
	url="$1"
	dest="$2"
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL -o "${dest}" "${url}"
	else
		wget -qO "${dest}" "${url}"
	fi
}

target=$(detect_target)
tag=$(latest_tag)
archive="${bin}-${tag}-${target}.tar.gz"
url="https://github.com/${repo}/releases/download/${tag}/${archive}"

echo "installing ${bin} ${tag} (${target})"

tmpdir=$(mktemp -d)
trap 'rm -rf "${tmpdir}"' EXIT

download "${url}" "${tmpdir}/${archive}"
tar -xzf "${tmpdir}/${archive}" -C "${tmpdir}"

mkdir -p "${install_dir}"
mv "${tmpdir}/${bin}" "${install_dir}/${bin}"
chmod +x "${install_dir}/${bin}"
ln -sf "${install_dir}/${bin}" "${install_dir}/ub"

echo "installed ${install_dir}/${bin}"
echo "created alias ${install_dir}/ub"

# --- install shell completions ---

completion_dir="${HOME}/.local/share/kagaya/completions"
mkdir -p "${completion_dir}"

for shell in bash zsh fish; do
	completion_url="https://raw.githubusercontent.com/${repo}/${tag}/completions/ub.${shell}"
	download "${completion_url}" "${completion_dir}/ub.${shell}" 2>/dev/null || true
done

if [ -f "${completion_dir}/ub.bash" ]; then
	echo
	echo "shell completions installed to ${completion_dir}"
	echo
	echo "enable tab completion:"
	echo
	echo "  bash:"
	echo "    echo 'source ${completion_dir}/ub.bash' >> ~/.bashrc"
	echo
	echo "  zsh:"
	echo "    echo 'fpath=(${completion_dir} \$fpath)' >> ~/.zshrc"
	echo "    echo 'autoload -Uz compinit && compinit' >> ~/.zshrc"
	echo
	echo "  fish:"
	echo "    ln -s ${completion_dir}/ub.fish ~/.config/fish/completions/"
fi

# --- PATH hint ---

if ! echo "${PATH}" | tr ':' '\n' | grep -qx "${install_dir}"; then
	echo
	echo "add to your PATH:"
	echo "  export PATH=\"${install_dir}:\${PATH}\""
fi
