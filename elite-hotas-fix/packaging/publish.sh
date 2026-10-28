#!/usr/bin/env bash
# Build and publish one tagged version of elite-hotas-fix to GitHub:
# a release with the tarball, the Arch/Debian/RPM packages and SHA256SUMS.
# Safe to run again: files already published unchanged are skipped, and a
# changed file replaces its predecessor on the release.
#
#   git tag -a v1.0.1 -m "elite-hotas-fix 1.0.1" && git push origin main v1.0.1
#   packaging/publish.sh                     # version from Cargo.toml
#   packaging/publish.sh 1.0.1 --notes notes.md
#   packaging/publish.sh --dist              # publish existing dist/ as is
#   packaging/publish.sh --dry-run           # show what would happen
#
# Needs: jq, and the build tools of build-packages.sh (unless --dist).
# Token: $GITHUB_TOKEN or $GH_TOKEN, or the file ~/.config/github-token.
# A classic token needs the "public_repo" scope; a fine-grained token needs
# "Contents: read and write" on this repository. Only needed for publishing,
# not for --dry-run.
#
# Note: GitHub has no apt/dnf/pacman package registries, so this script only
# publishes the release itself. See RELEASE.md for where each package goes.
set -euo pipefail

owner=${OWNER:-LunaBaloona}
repo=${REPO:-Virpil_devices_on_Linux}

here=$(cd "$(dirname "$0")" && pwd)
tool=$(dirname "$here")
dist=$tool/dist

version= notes= dry= do_build=1
while (($#)); do
  case $1 in
    --notes) notes=$2; shift 2 ;;
    --dist) do_build=; shift ;;
    -n|--dry-run) dry=1; shift ;;
    -h|--help) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    -*) echo "unknown option $1" >&2; exit 2 ;;
    *) version=$1; shift ;;
  esac
done
version=${version:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$tool/Cargo.toml" | head -1)}
tag=v$version
gh_api=${GITHUB_API_URL:-https://api.github.com}          # set for GitHub Enterprise
gh_upload=${GITHUB_UPLOAD_URL:-${gh_api/api./uploads.}}
api=$gh_api/repos/$owner/$repo
upload=$gh_upload/repos/$owner/$repo

command -v jq >/dev/null || { echo "jq is required (apt install jq / pacman -S jq)" >&2; exit 2; }
[[ -z $notes || -r $notes ]] || { echo "can't read notes file $notes" >&2; exit 2; }

token=${GITHUB_TOKEN:-${GH_TOKEN:-}}
if [[ -z $token ]]; then
  for f in ~/.config/github-token ~/.config/gitea-token; do
    [[ -r $f ]] && { token=$(tr -d '[:space:]' < "$f"); break; }
  done
fi
if [[ -z $token && -z $dry ]]; then
  echo "no token: set GITHUB_TOKEN or write ~/.config/github-token" >&2; exit 2
fi

local_tag=$(git -C "$tool" rev-parse -q --verify "refs/tags/$tag^{commit}") ||
  { echo "no tag $tag; tag the release first" >&2; exit 2; }
# ls-remote lists an annotated tag's object first and the peeled commit on a
# "tag^{}" line; compare commits, so prefer the peeled line and fall back to
# the ref line for plain (non-annotated) tags.
lsr=$(git -C "$tool" ls-remote origin "refs/tags/$tag^{}" "refs/tags/$tag" || true)
remote_tag=$(awk -v r="refs/tags/$tag^{}" '$2==r {print $1; exit}' <<<"$lsr")
[[ -n $remote_tag ]] ||
  remote_tag=$(awk -v r="refs/tags/$tag" '$2==r {print $1; exit}' <<<"$lsr")
[[ -n $remote_tag && $remote_tag == "$local_tag" ]] ||
  { echo "tag $tag is not on origin (or differs); push it first: git push origin $tag" >&2; exit 2; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
reljson=$work/rel.json
run() { if [[ -n $dry ]]; then echo "   (dry run) $*"; else "$@"; fi; }
# Without a token (allowed for --dry-run on a public repo) fall back to plain GETs.
auth() {
  if [[ -n $token ]]; then curl -sS --max-time 120 -H "Authorization: Bearer $token" "$@"
  else curl -sS --max-time 120 "$@"; fi
}
# GitHub wants the raw file as the POST body (Gitea took form fields).
upload_asset() {
  local code
  code=$(auth -o /dev/null -w '%{http_code}' -X POST -T "$1" \
    -H 'Content-Type: application/octet-stream' "$upload/releases/$id/assets?name=$1")
  [[ $code == 201 ]] || { echo "upload of $1 failed: HTTP $code" >&2; exit 1; }
}
sha() { sha256sum "$1" | cut -d' ' -f1; }

# Compare a published copy at $2 with the local file $1.
# Returns 0 if identical, 1 if it is not published or differs.
published_same() {
  local tmp
  tmp=$(mktemp)
  if curl -sfL --max-time 120 -o "$tmp" "$2" && [[ $(sha "$tmp") == $(sha "$1") ]]; then
    rm -f "$tmp"; return 0
  fi
  rm -f "$tmp"; return 1
}

if [[ -n $do_build ]]; then
  echo "==> Building $tag"
  "$here/build-packages.sh" "$version"
fi
cd "$dist"
tarball=elite-hotas-fix-$version-x86_64-linux.tar.gz
pkg_arch=$(echo elite-hotas-fix-"$version"-*-x86_64.pkg.tar.zst)
pkg_deb=$(echo elite-hotas-fix_"$version"-*_amd64.deb)
pkg_rpm=$(echo elite-hotas-fix-"$version"-*.x86_64.rpm)

# The Arch package is optional: only machines with makepkg produce it.
files=("$tarball")
[[ -f $pkg_arch ]] || echo "!! no $pkg_arch; publishing without the Arch package" >&2
for f in "$pkg_arch" "$pkg_deb" "$pkg_rpm"; do [[ -f $f ]] && files+=("$f"); done
[[ -f SHA256SUMS ]] || { echo "missing dist/SHA256SUMS" >&2; exit 1; }
for f in "${files[@]}" SHA256SUMS; do
  [[ -f $f ]] || { echo "missing dist/$f" >&2; exit 1; }
done

echo "==> Release $tag"
code=$(auth -o "$reljson" -w '%{http_code}' "$api/releases/tags/$tag")
if [[ $code == 404 ]]; then
  body=${notes:+$(cat "$notes")}
  body=${body:-elite-hotas-fix $version. Install and usage steps are in the [README](https://github.com/$owner/$repo/blob/$tag/elite-hotas-fix/README.md).}
  if [[ -n $dry ]]; then
    echo "   (dry run) create release $tag"
    id=dry-run
    echo '{"assets":[]}' > "$reljson"
  else
    id=$(jq -n --arg tag "$tag" --arg name "elite-hotas-fix $version" --arg body "$body" \
           '{tag_name:$tag,name:$name,body:$body,draft:false,prerelease:false}' |
         auth -f -H 'Content-Type: application/json' -d @- "$api/releases" | jq -r .id)
    echo "   created release $id"
    code=$(auth -o "$reljson" -w '%{http_code}' "$api/releases/tags/$tag")
  fi
elif [[ $code == 200 ]]; then
  id=$(jq -r .id "$reljson")
  echo "   release $id exists"
  [[ -z $notes ]] || echo "   (--notes ignored: edit the existing release's notes on github)"
else
  echo "release lookup failed: HTTP $code" >&2; exit 1
fi

for f in "${files[@]}" SHA256SUMS; do
  old=$(jq -r --arg n "$f" '.assets[]? | select(.name==$n) | .id' "$reljson" 2>/dev/null || true)
  if [[ -n $old ]]; then
    url=$(jq -r --arg n "$f" '.assets[]? | select(.name==$n) | .browser_download_url' "$reljson")
    if published_same "$f" "$url"; then
      echo "   = $f (already attached)"; continue
    fi
    echo "   ~ $f changed, replacing it"
    run auth -o /dev/null -X DELETE "$api/releases/assets/$old"
  fi
  echo "   + $f"
  if [[ -n $dry ]]; then echo "   (dry run) upload $f"; else upload_asset "$f"; fi
done

[[ -n $dry ]] && { echo "==> Dry run finished; nothing was changed"; exit 0; }

echo "==> Checking public downloads"
check=$(mktemp -d)
trap 'rm -rf "$work" "$check"' EXIT
curl -sfL --max-time 120 -o "$check/rel.json" "$api/releases/tags/$tag"
for f in "${files[@]}" SHA256SUMS; do
  url=$(jq -r --arg n "$f" '.assets[] | select(.name==$n) | .browser_download_url' "$check/rel.json")
  [[ $url != null && -n $url ]] || { echo "!! $f is not attached to the release" >&2; exit 1; }
  curl -sfL --max-time 120 -o "$check/$f" "$url"
done
(cd "$check" && sha256sum -c --quiet --ignore-missing SHA256SUMS) &&
  echo "   all release files verified"
echo "==> Published $tag: https://github.com/$owner/$repo/releases/tag/$tag"
