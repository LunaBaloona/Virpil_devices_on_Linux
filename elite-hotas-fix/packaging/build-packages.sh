#!/usr/bin/env bash
# Build the release tarball and the Arch, Debian and RPM packages for one
# tagged version of elite-hotas-fix. Everything is built from the git tag
# (v<version>), not the working tree, so the files match the release.
#
#   packaging/build-packages.sh            # version from Cargo.toml
#   packaging/build-packages.sh 1.0.0
#
# Needs: cargo with the x86_64-unknown-linux-musl target, makepkg, dpkg-deb,
# rpmbuild. Output goes to elite-hotas-fix/dist/.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tool=$(dirname "$here")
repo=$(git -C "$tool" rev-parse --show-toplevel)

version=${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$tool/Cargo.toml" | head -1)}
tag=v$version
name=elite-hotas-fix
base=$name-$version-x86_64-linux
dist=$tool/dist

git -C "$repo" rev-parse -q --verify "refs/tags/$tag" >/dev/null ||
  { echo "no tag $tag; tag the release first" >&2; exit 2; }
skip=
for tool_ in dpkg-deb rpmbuild makepkg; do
  command -v "$tool_" >/dev/null || { echo "!! $tool_ not found; that package will be skipped" >&2; skip+=" $tool_"; }
done
for f in "$here/arch/PKGBUILD" "$here/rpm/$name.spec"; do
  grep -Eq "^(pkgver=|Version: +)$version\$" "$f" ||
    { echo "$f is not at version $version" >&2; exit 2; }
done

export SOURCE_DATE_EPOCH=$(git -C "$repo" log -1 --format=%ct "$tag")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
rm -rf "$dist"
mkdir -p "$dist"

echo "==> Source from $tag"
git -C "$repo" archive "$tag" | tar -x -C "$work" --one-top-level=src
src=$work/src/elite-hotas-fix

echo "==> Static musl binary"
cargo build --quiet --release --locked --target x86_64-unknown-linux-musl \
  --manifest-path "$src/Cargo.toml" --target-dir "$work/target"
bin=$work/target/x86_64-unknown-linux-musl/release/$name

echo "==> Release tarball"
mkdir "$work/$base"
install -m755 "$bin" "$work/$base/$name"
install -m644 "$src/README.md" "$work/src/LICENSE" "$work/$base/"
tar -C "$work" --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 \
  --numeric-owner -cf - "$base" | gzip -n9 > "$dist/$base.tar.gz"

echo "==> Debian package"
if [[ $skip == *dpkg-deb* ]]; then echo "   (skipped)"; else
deb=$work/deb
install -Dm755 "$bin" "$deb/usr/bin/$name"
install -Dm644 "$src/README.md" "$deb/usr/share/doc/$name/README.md"
install -Dm644 "$here/debian/copyright" "$deb/usr/share/doc/$name/copyright"
mkdir -p "$deb/DEBIAN"
size=$(du -sk --exclude=DEBIAN "$deb" | cut -f1)
sed -e "s/@VERSION@/$version/" -e "s/@SIZE@/$size/" "$here/debian/control" \
  > "$deb/DEBIAN/control"
dpkg-deb --root-owner-group -Zxz --build "$deb" "$dist/${name}_$version-1_amd64.deb" >/dev/null
fi

echo "==> RPM package"
if [[ $skip == *rpmbuild* ]]; then echo "   (skipped)"; else
# --nodeps: there are no build requirements, and the build host (often Arch)
# has no RPM database to check them against.
rpmbuild --quiet --nodeps -bb "$here/rpm/$name.spec" \
  --define "_topdir $work/rpm" --define "_sourcedir $dist" \
  --define "use_source_date_epoch_as_buildtime 1" \
  --define "build_mtime_policy clamp_to_source_date_epoch"
cp "$work"/rpm/RPMS/x86_64/*.rpm "$dist/"
fi

echo "==> Arch package"
if [[ $skip == *makepkg* ]]; then echo "   (skipped)"; else
# The PKGBUILD downloads its source from the tag on github, so the tag must
# be pushed before building, and only now can its archive be hashed.
arch_url=$(sed -n 's/^url="\(.*\)"$/\1/p' "$here/arch/PKGBUILD" | head -1)
git -C "$repo" ls-remote --exit-code origin "refs/tags/$tag" >/dev/null 2>&1 ||
  { echo "tag $tag is not on origin; push it first: git push origin $tag" >&2; exit 2; }
mkdir "$work/arch"
cp "$here/arch/PKGBUILD" "$work/arch/"
arch_sha=$(curl -fsSL --max-time 120 "$arch_url/archive/v$version.tar.gz" | sha256sum | cut -d' ' -f1)
sed -i "s/^sha256sums=.*/sha256sums=('$arch_sha')/" "$work/arch/PKGBUILD"
(cd "$work/arch" && PKGDEST="$dist" makepkg --noconfirm --cleanbuild >"$work/makepkg.log" 2>&1) ||
  { cat "$work/makepkg.log" >&2; echo "makepkg failed" >&2; exit 1; }
fi

(cd "$dist" && find . -maxdepth 1 -type f \( -name '*.tar.gz' -o -name '*.deb' -o \
  -name '*.rpm' -o -name '*.pkg.tar.zst' \) -printf '%P\n' | sort | xargs -r sha256sum > SHA256SUMS)
[[ -s $dist/SHA256SUMS ]] || { echo "nothing was built" >&2; exit 1; }
echo "==> Done${skip:+; skipped:$skip}"
ls -l "$dist"
