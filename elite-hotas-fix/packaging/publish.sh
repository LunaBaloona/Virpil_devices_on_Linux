#!/usr/bin/env bash
# Build and publish one tagged version of elite-hotas-fix to github:
# the release (tarball, packages, SHA256SUMS) and the pacman, apt and dnf
# package registries. Safe to run again: files already published unchanged
# are skipped, and a published file that differs stops the script.
#
#   git tag -a v1.0.1 -m "elite-hotas-fix 1.0.1" && git push origin main v1.0.1
#   packaging/publish.sh                     # version from Cargo.toml
#   packaging/publish.sh 1.0.1 --notes notes.md
#   packaging/publish.sh --dry-run           # show what would happen
#
# Token: $GITEA_TOKEN, or the file ~/.config/gitea-token. It needs the
# write:repository and write:package scopes.
set -euo pipefail

host=${GITEA_URL:-https://github.com/LunaBaloona/Virpil_devices_on_Linux}
owner=${GITEA_OWNER:-LunaBaloona}
repo=${GITEA_REPO:-Virpil_devices_on_Linux}
arch_repo=virpil              # pacman repository: [virpil]
deb_dist=stable deb_comp=main # apt: "stable main"

here=$(cd "$(dirname "$0")" && pwd)
tool=$(dirname "$here")
dist=$tool/dist

version= notes= dry=
while (($#)); do
  case $1 in
    --notes) notes=$2; shift 2 ;;
    -n|--dry-run) dry=1; shift ;;
    -h|--help) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    -*) echo "unknown option $1" >&2; exit 2 ;;
    *) version=$1; shift ;;
  esac
done
version=${version:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$tool/Cargo.toml" | head -1)}
tag=v$version
api=$host/api/v1/repos/$owner/$repo
pkgs=$host/api/packages/$owner

token=${GITEA_TOKEN:-$(tr -d '[:space:]' < ~/.config/gitea-token 2>/dev/null || true)}
[[ -n $token ]] || { echo "no token: set GITEA_TOKEN or write ~/.config/gitea-token" >&2; exit 2; }
[[ -z $notes || -r $notes ]] || { echo "can't read notes file $notes" >&2; exit 2; }

local_tag=$(git -C "$tool" rev-parse -q --verify "refs/tags/$tag^{commit}") ||
  { echo "no tag $tag; tag the release first" >&2; exit 2; }
remote_tag=$(git -C "$tool" ls-remote origin "refs/tags/$tag^{}" | cut -f1)
[[ $remote_tag == "$local_tag" ]] ||
  { echo "tag $tag is not on origin (or differs); push it first: git push origin $tag" >&2; exit 2; }

run() { if [[ -n $dry ]]; then echo "   (dry run) $*"; else "$@"; fi; }
auth() { curl -sS --max-time 120 -H "Authorization: token $token" "$@"; }
sha() { sha256sum "$1" | cut -d' ' -f1; }

# Compare a published copy at $2 with the local file $1.
# Returns 0 if identical, 1 if it isn't published, and exits if it differs.
published_same() {
  local tmp code
  tmp=$(mktemp)
  code=$(curl -sL --max-time 120 -o "$tmp" -w '%{http_code}' "$2")
  if [[ $code != 200 ]]; then rm -f "$tmp"; return 1; fi
  if [[ $(sha "$tmp") == $(sha "$1") ]]; then rm -f "$tmp"; return 0; fi
  rm -f "$tmp"
  echo "!! $2 is already published with different contents." >&2
  echo "   Bump the package release (pkgrel / -1) or the version instead of overwriting it." >&2
  exit 1
}

# makepkg records the build directory in .BUILDINFO (and so .MTREE), so two
# builds of the same Arch package differ only there. Compare everything else.
same_arch_pkg() {
  local d r=0
  d=$(mktemp -d)
  mkdir "$d/a" "$d/b"
  bsdtar -xf "$1" -C "$d/a" --exclude .BUILDINFO --exclude .MTREE &&
    bsdtar -xf "$2" -C "$d/b" --exclude .BUILDINFO --exclude .MTREE &&
    diff -r "$d/a" "$d/b" >/dev/null || r=1
  rm -rf "$d"
  return $r
}

echo "==> Building $tag"
"$here/build-packages.sh" "$version"
cd "$dist"
tarball=elite-hotas-fix-$version-x86_64-linux.tar.gz
pkg_arch=$(echo elite-hotas-fix-"$version"-*-x86_64.pkg.tar.zst)
pkg_deb=$(echo elite-hotas-fix_"$version"-*_amd64.deb)
pkg_rpm=$(echo elite-hotas-fix-"$version"-*.x86_64.rpm)
for f in "$tarball" "$pkg_arch" "$pkg_deb" "$pkg_rpm" SHA256SUMS; do
  [[ -f $f ]] || { echo "missing dist/$f" >&2; exit 1; }
done

# If this Arch package is already published and differs only in build
# metadata, keep the published file so its checksum stays the same.
pub=$(mktemp)
if curl -sfL --max-time 120 -o "$pub" "$pkgs/arch/$arch_repo/x86_64/$pkg_arch" &&
   [[ $(sha "$pub") != $(sha "$pkg_arch") ]] && same_arch_pkg "$pub" "$pkg_arch"; then
  echo "   using the published $pkg_arch (only its build metadata differed)"
  cp "$pub" "$pkg_arch"
  sha256sum -- *.tar.gz *.deb *.rpm *.pkg.tar.zst > SHA256SUMS
fi
rm -f "$pub"

echo "==> Release $tag"
code=$(auth -o release.json -w '%{http_code}' "$api/releases/tags/$tag")
if [[ $code == 404 ]]; then
  body=${notes:+$(cat "$notes")}
  body=${body:-elite-hotas-fix $version. Install and usage steps are in the [README]($host/$owner/$repo/src/tag/$tag/elite-hotas-fix/README.md).}
  if [[ -n $dry ]]; then
    echo "   (dry run) create release $tag"; id=
  else
    id=$(jq -n --arg tag "$tag" --arg name "elite-hotas-fix $version" --arg body "$body" \
           '{tag_name:$tag,name:$name,body:$body,draft:false,prerelease:false}' |
         auth -f -H 'Content-Type: application/json' -d @- "$api/releases" | jq -r .id)
    echo "   created release $id"
    echo '{"assets":[]}' > release.json
  fi
elif [[ $code == 200 ]]; then
  id=$(jq -r .id release.json)
  echo "   release $id exists"
  [[ -z $notes ]] || echo "   (--notes ignored: edit the existing release's notes)"
else
  echo "release lookup failed: HTTP $code" >&2; exit 1
fi

for f in "$tarball" "$pkg_arch" "$pkg_deb" "$pkg_rpm" SHA256SUMS; do
  old=$(jq -r --arg n "$f" '.assets[]? | select(.name==$n) | .id' release.json 2>/dev/null || true)
  if [[ -n $old ]]; then
    if published_same "$f" "$host/$owner/$repo/releases/download/$tag/$f"; then
      echo "   = $f (already attached)"; continue
    fi
  fi
  echo "   + $f"
  run auth -f -o /dev/null -F "attachment=@$f" "$api/releases/$id/assets?name=$f"
done
rm -f release.json

echo "==> Package registries"
publish_pkg() { # file, upload URL, download URL
  if published_same "$1" "$3"; then echo "   = $1 (already published)"; return; fi
  echo "   + $1"
  [[ -n $dry ]] && { echo "   (dry run) PUT $2"; return; }
  local code
  code=$(auth -o /dev/null -w '%{http_code}' --upload-file "$1" "$2")
  [[ $code == 201 ]] || { echo "upload of $1 failed: HTTP $code" >&2; exit 1; }
}
rpm_ver=${pkg_rpm#elite-hotas-fix-}; rpm_ver=${rpm_ver%.x86_64.rpm}
publish_pkg "$pkg_arch" "$pkgs/arch/$arch_repo" "$pkgs/arch/$arch_repo/x86_64/$pkg_arch"
publish_pkg "$pkg_deb" "$pkgs/debian/pool/$deb_dist/$deb_comp/upload" "$pkgs/debian/pool/$deb_dist/$deb_comp/$pkg_deb"
publish_pkg "$pkg_rpm" "$pkgs/rpm/upload" "$pkgs/rpm/package/elite-hotas-fix/$rpm_ver/x86_64/$pkg_rpm"

[[ -n $dry ]] && { echo "==> Dry run finished; nothing was changed"; exit 0; }

echo "==> Checking public downloads"
check=$(mktemp -d)
trap 'rm -rf "$check"' EXIT
for f in "$tarball" "$pkg_arch" "$pkg_deb" "$pkg_rpm" SHA256SUMS; do
  curl -sfL --max-time 120 -o "$check/$f" "$host/$owner/$repo/releases/download/$tag/$f"
done
(cd "$check" && sha256sum -c --quiet SHA256SUMS) && echo "   release files OK"
for u in "$pkgs/arch/$arch_repo/x86_64/$pkg_arch" "$pkgs/debian/pool/$deb_dist/$deb_comp/$pkg_deb" \
         "$pkgs/rpm/package/elite-hotas-fix/$rpm_ver/x86_64/$pkg_rpm"; do
  published_same "$dist/$(basename "$u")" "$u" || { echo "!! not served: $u" >&2; exit 1; }
done
echo "   registry packages OK"
echo "==> Published $tag: $host/$owner/$repo/releases/tag/$tag"
