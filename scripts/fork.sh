#!/usr/bin/env bash
# Build, install, and publish this fork.
#
# The fork's release identity is the tag in FORK_VERSION (fork-<upstream version>-<n>).
# Installed builds live side by side, one directory per release, and `herdr` on PATH is a
# symlink to one of them, so building never changes the binary in use and switching back is
# one command. Nothing here stops a running Herdr server: sessions keep running on the build
# they started with until that server is restarted.
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: scripts/fork.sh <command>

  install        build the current tree and switch `herdr` to it
                 (a clean checkout of the FORK_VERSION tag installs as that release,
                  anything else installs as "dev" and replaces the previous dev build)
  list           list installed builds and mark the one in use
  use <name>     switch `herdr` to an installed build, e.g. fork-0.9.5-1 or dev
  publish        tag FORK_VERSION, build macOS here and Linux on GitHub Actions,
                 create the GitHub release, then install it

environment:
  HERDR_FORK_HOME   where builds are kept (default: ~/.local/share/herdr-fork)
  HERDR_FORK_BIN    the symlink to manage (default: ~/.local/bin/herdr)
  ZIG               Zig 0.16.0 binary (default: ~/.local/share/herdr/zig-0.16.0/zig if present)
USAGE
}

repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

fork_home="${HERDR_FORK_HOME:-$HOME/.local/share/herdr-fork}"
bin_link="${HERDR_FORK_BIN:-$HOME/.local/bin/herdr}"
tag="$(tr -d '[:space:]' < FORK_VERSION)"
[[ "$tag" == fork-* ]] || { echo "FORK_VERSION must hold a fork-* tag, found: $tag" >&2; exit 1; }

die() { echo "error: $*" >&2; exit 1; }

toolchain_env() {
  if [[ -d /opt/homebrew/opt/rustup/bin ]]; then
    PATH="/opt/homebrew/opt/rustup/bin:$PATH"
  fi
  if [[ -z "${ZIG:-}" && -x "$HOME/.local/share/herdr/zig-0.16.0/zig" ]]; then
    export ZIG="$HOME/.local/share/herdr/zig-0.16.0/zig"
  fi
  command -v cargo >/dev/null || die "cargo not found"
}

build_release() {
  toolchain_env
  # Cargo may need the proxy to fetch crates. Zig's package fetch fails through an HTTP
  # proxy, so the build itself runs without one.
  cargo fetch --locked >/dev/null
  env -u ALL_PROXY -u HTTPS_PROXY -u HTTP_PROXY -u all_proxy -u http_proxy -u https_proxy \
    cargo build --release --locked --offline
}

# Name the build of the current tree: the release tag for a clean checkout of it, else "dev".
build_name() {
  if [[ -z "$(git status --porcelain)" ]] \
    && [[ "$(git rev-parse -q --verify "refs/tags/$tag^{commit}" 2>/dev/null)" == "$(git rev-parse HEAD)" ]]; then
    echo "$tag"
  else
    echo "dev"
  fi
}

switch_to() {
  local name="$1" target="$fork_home/$1/herdr"
  [[ -x "$target" ]] || die "no installed build named $name (see: scripts/fork.sh list)"
  mkdir -p "$(dirname "$bin_link")"
  if [[ -e "$bin_link" && ! -L "$bin_link" ]]; then
    die "$bin_link exists and is not a symlink; move it away first"
  fi
  # Replace the link in one rename so there is never a moment without a herdr on PATH.
  ln -s "$target" "$bin_link.new.$$"
  mv -f "$bin_link.new.$$" "$bin_link"
  echo "herdr -> $name ($("$bin_link" --version))"
}

cmd_install() {
  build_release
  local name dest
  name="$(build_name)"
  dest="$fork_home/$name"
  if [[ "$name" != dev && -x "$dest/herdr" ]]; then
    echo "$name is already installed"
  else
    mkdir -p "$dest"
    # A new file, then a rename: a server still running the old dev build keeps its own copy.
    cp target/release/herdr "$dest/herdr.new.$$"
    mv -f "$dest/herdr.new.$$" "$dest/herdr"
    git rev-parse HEAD > "$dest/COMMIT"
  fi
  switch_to "$name"
  echo "a running server keeps its current build until it is restarted"
}

cmd_list() {
  local current=""
  [[ -L "$bin_link" ]] && current="$(readlink "$bin_link")"
  [[ -d "$fork_home" ]] || { echo "no builds installed under $fork_home"; return; }
  local dir name mark
  for dir in "$fork_home"/*/; do
    [[ -x "$dir/herdr" ]] || continue
    name="$(basename "$dir")"
    mark=" "
    [[ "$current" == "$fork_home/$name/herdr" ]] && mark="*"
    printf '%s %-18s %s\n' "$mark" "$name" "$(cut -c1-8 "$dir/COMMIT" 2>/dev/null || true)"
  done
  if [[ -n "$current" && "$current" != "$fork_home"/* ]]; then
    echo "* (outside $fork_home) $current"
  fi
}

cmd_publish() {
  command -v gh >/dev/null || die "gh not found"
  local slug head
  slug="$(git remote get-url origin | sed -E 's#(git@github.com:|https://github.com/)##; s#\.git$##')"
  head="$(git rev-parse HEAD)"
  [[ "$(git branch --show-current)" == master ]] || die "publish from master"
  [[ -z "$(git status --porcelain)" ]] || die "working tree is not clean"
  git fetch -q origin master
  [[ "$(git rev-parse origin/master)" == "$head" ]] || die "master is not pushed to origin"
  if git rev-parse -q --verify "refs/tags/$tag" >/dev/null || [[ -n "$(git ls-remote --tags origin "$tag")" ]]; then
    die "$tag already exists; bump FORK_VERSION first"
  fi

  echo "== building macOS"
  build_release
  ./target/release/herdr --version | grep -qF "($tag)" || die "built binary does not report $tag"
  local arch asset_mac work
  arch="$(uname -m)"; [[ "$arch" == arm64 ]] && arch=aarch64
  asset_mac="herdr-macos-$arch"
  work="$(mktemp -d)"
  cp target/release/herdr "$work/$asset_mac"

  echo "== tagging $tag"
  git tag -a "$tag" -m "$tag"
  git push -q origin "$tag"

  echo "== building Linux on GitHub Actions"
  local started run
  started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  gh workflow run "Build artifacts (manual)" --repo "$slug" --ref "$tag" \
    -f build_group=linux -f libghostty_optimize=ReleaseFast -f libghostty_simd=true >/dev/null
  run=""
  for _ in $(seq 1 30); do
    run="$(gh run list --repo "$slug" --workflow "Build artifacts (manual)" --limit 5 \
      --json databaseId,headSha,createdAt \
      -q "[.[] | select(.headSha == \"$head\" and .createdAt >= \"$started\")][0].databaseId")"
    [[ -n "$run" ]] && break
    sleep 4
  done
  [[ -n "$run" ]] || die "could not find the workflow run; the tag is pushed, finish the release by hand"
  gh run watch "$run" --repo "$slug" --interval 20 --exit-status >/dev/null
  gh run download "$run" --repo "$slug" --dir "$work/linux"
  local a info
  for a in x86_64 aarch64; do
    info="$(echo "$work"/linux/herdr-linux-"$a"-*/BUILD_INFO.txt)"
    grep -qx "commit=$head" "$info" || die "linux $a was not built from $head"
    cp "$(dirname "$info")/herdr-linux-$a" "$work/"
  done

  echo "== creating the release"
  (cd "$work" && for a in "$asset_mac" herdr-linux-x86_64 herdr-linux-aarch64; do
    shasum -a 256 "$a" > "$a.sha256"
  done)
  local previous
  previous="$(git describe --tags --abbrev=0 --match 'fork-*' "$tag^" 2>/dev/null || true)"
  {
    echo "Unofficial build of a personal fork of [herdrdev/herdr](https://github.com/herdrdev/herdr). It is not affiliated with or supported by the Herdr maintainers."
    echo
    echo "What the fork changes is described in the [README](https://github.com/$slug#this-fork)."
    echo
    if [[ -n "$previous" ]]; then
      echo "## Changes since $previous"
      echo
      git log --no-merges --format='- %s' "$previous..$tag"
      echo
    fi
    echo "## Install"
    echo
    echo '```bash'
    echo "asset=herdr-linux-x86_64   # or herdr-linux-aarch64, $asset_mac"
    echo 'mkdir -p ~/.local/bin'
    echo "curl -fsSL -o ~/.local/bin/herdr \"https://github.com/$slug/releases/download/$tag/\$asset\""
    echo 'chmod +x ~/.local/bin/herdr'
    echo '```'
    echo
    echo '`herdr --version` reports the fork release in parentheses. Checksums (SHA-256):'
    echo
    echo '```text'
    cat "$work"/*.sha256
    echo '```'
    echo
    echo "The Linux binaries are statically linked against musl. The macOS binary is ad-hoc signed, not notarized: download it with \`curl\`, or clear the quarantine flag on a browser download. \`herdr update\` installs the official release and replaces this build."
  } > "$work/notes.md"
  gh release create "$tag" --repo "$slug" --verify-tag --title "$tag" --notes-file "$work/notes.md" \
    "$work/$asset_mac" "$work/$asset_mac.sha256" \
    "$work/herdr-linux-x86_64" "$work/herdr-linux-x86_64.sha256" \
    "$work/herdr-linux-aarch64" "$work/herdr-linux-aarch64.sha256"
  rm -rf "$work"

  echo "== installing $tag"
  cmd_install
}

case "${1:-}" in
  install) cmd_install ;;
  list) cmd_list ;;
  use) [[ $# -eq 2 ]] || { usage >&2; exit 2; }; switch_to "$2" ;;
  publish) cmd_publish ;;
  -h|--help|help) usage ;;
  *) usage >&2; exit 2 ;;
esac
