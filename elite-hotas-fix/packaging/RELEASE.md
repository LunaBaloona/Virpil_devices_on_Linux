# Making a release

Every release is built from a git tag and published as a GitHub Release with
these files attached:

| File | For | Built by |
|---|---|---|
| `elite-hotas-fix-<v>-x86_64-linux.tar.gz` | any x86_64 distro | `build-packages.sh` |
| `elite-hotas-fix_<v>-1_amd64.deb` | Debian, Ubuntu, Mint, Pop!_OS | `build-packages.sh` |
| `elite-hotas-fix-<v>-1.x86_64.rpm` | Fedora, openSUSE, RHEL-family | `build-packages.sh` |
| `elite-hotas-fix-<v>-1-x86_64.pkg.tar.zst` | Arch | `makepkg` (Arch only) |
| `SHA256SUMS` | checksums of the four files above | `build-packages.sh` |

## One-off machine requirements

One machine needs cargo with the `x86_64-unknown-linux-musl` target plus
`makepkg`, `dpkg-deb` and `rpmbuild`, so an Arch box with

    sudo pacman -S --needed rust dpkg rpm-tools jq

is the practical choice. (On any distro you can instead build tarball + deb +
rpm and skip `makepkg`; the script says what it skipped. See *Partial sets*
below.) Rust needs the musl target once: with rustup
`rustup target add x86_64-unknown-linux-musl`; on Arch `pacman -S rust`
provides it through rustup.

## The release steps

Below for version `X.Y.Z`. `publish.sh` does steps 4–6 itself; steps 1–3 are
the part you do by hand.

1. **Check the version** is the new one in all three files, and bump if not:
   `Cargo.toml` (`version`), `packaging/arch/PKGBUILD` (`pkgver`),
   `packaging/rpm/elite-hotas-fix.spec` (`Version` + a `%changelog` entry).
   Commit.

2. **Tag and push.** Everything builds from the tag, and the Arch package
   downloads its source from it, so the tag must be on github before building:

       git tag -a vX.Y.Z -m "elite-hotas-fix X.Y.Z"
       git push origin main vX.Y.Z

3. **Choose notes** (optional): write release notes to a file, e.g.
   `notes.md`. Without it the release body is a default line pointing at the
   README.

4. **Build and publish:**

       packaging/publish.sh --notes notes.md      # or without --notes

   The script builds everything into `elite-hotas-fix/dist/`, creates
   `releases/tag/vX.Y.Z` if missing, uploads the files, refuses to touch
   unchanged files, replaces files that changed, and finishes by downloading
   the release again and verifying every checksum. `--dry-run` first is a
   good habit; `--dist` publishes an existing `dist/` without rebuilding.

5. **Try the packages** you just published: install the deb in a container,
   `dnf install ./…rpm` or `rpm -qp --scripts` the rpm, and run the tarball
   binary on an Arch box.

6. **Tell the docs.** If the URLs in the READMEs point at `latest`, nothing
   to do; otherwise update them and the install instructions.

## Partial sets

Building without `makepkg` (e.g. on Debian or in CI) produces everything but
the Arch package, and `publish.sh` warns and proceeds. To add the Arch
package later, on an Arch box with the full toolchain:

    packaging/publish.sh            # rebuilds, then uploads only what changed

The binary is rebuilt from the same tag, so the tarball/deb/rpm are reuploaded
only if they actually differ.

## Where each package goes after publishing

GitHub releases serve all four files, but GitHub has no apt/dnf/pacman
package *registries*, so `apt install`, `dnf install` and `pacman -S` cannot
point at this repository directly:

- **Arch users**: `pacman -U` the downloaded `.pkg.tar.zst`, or install from
  the **AUR** once `packaging/aur/PKGBUILD` is submitted (see its header).
- **Debian users**: `sudo apt install ./elite-hotas-fix_*_amd64.deb`.
- **Fedora/openSUSE users**: `sudo dnf install ./elite-hotas-fix-*.x86_64.rpm`.

A real apt/dnf repository needs separate hosting (GitHub Pages with
reprepro/createrepo, Cloudsmith, or openSUSE OBS); that is a possible future
step, not part of a release.

## Rollback / re-release notes

- Never move a published tag. Fix forward with the next version, or edit the
  release text; rerunning `publish.sh` after a rebuild replaces changed
  assets automatically.
- If a release must be redone, delete the GitHub *release* (not the tag),
  rebuild from the same tag, and publish again. Users who already installed
  the deleted assets keep working installs; checksums change, so prefer a
  version bump.
