# AUR packaging

Two PKGBUILDs ship with the source tree:

- `btop-beautifier/` — builds from the tagged GitHub source tarball using `cargo`. Always reproducible; users without prebuilt-binary trust can compile locally.
- `btop-beautifier-bin/` — installs the prebuilt binary published to GitHub Releases by the `cargo-dist` workflow. Faster install for users who don't have a Rust toolchain.

The `provides=` and `conflicts=` arrays make the two mutually exclusive: a user picks one.

## First-time submission to AUR

You need an [AUR account](https://aur.archlinux.org/register) and an SSH key registered in your AUR profile.

```bash
# 1. Generate an AUR-only SSH key (skip if you already have one)
ssh-keygen -t ed25519 -f ~/.ssh/aur -C "aur"

# 2. Add SSH config so `git push` to AUR uses the right key
cat >> ~/.ssh/config <<'EOF'
Host aur.archlinux.org
  IdentityFile ~/.ssh/aur
  User aur
EOF
chmod 600 ~/.ssh/config

# 3. Paste ~/.ssh/aur.pub into your AUR profile:
#    https://aur.archlinux.org/account → "SSH Public Key"
```

For each PKGBUILD (do this twice — once for each package):

```bash
# Replace <pkgname> with btop-beautifier or btop-beautifier-bin
git clone ssh://aur@aur.archlinux.org/<pkgname>.git /tmp/aur-<pkgname>
cp pkg/aur/<pkgname>/PKGBUILD /tmp/aur-<pkgname>/

cd /tmp/aur-<pkgname>

# Build locally to confirm the PKGBUILD works (also tests dependencies)
makepkg -si

# Generate .SRCINFO (AUR requires this committed alongside PKGBUILD)
makepkg --printsrcinfo > .SRCINFO

git add PKGBUILD .SRCINFO
git commit -m "Initial release v0.1.0"
git push
```

## Updating sha256sums after a new release

`btop-beautifier-bin/PKGBUILD` ships with `sha256sums=('SKIP')` until release artifacts exist, then those need to be replaced with real hashes. The `updpkgsums` tool from the `pacman-contrib` package does this automatically:

```bash
sudo pacman -S pacman-contrib   # one-time
cd pkg/aur/btop-beautifier-bin
updpkgsums   # rewrites sha256sums_x86_64 and sha256sums_aarch64 in place
```

`btop-beautifier/PKGBUILD` (source build) has `sha256sums=('SKIP')` deliberately — `cargo`'s `--locked` already hashes everything reproducibly via `Cargo.lock`, so the tarball hash adds no real protection. You can change it to a real hash with `updpkgsums` if you prefer the AUR-conventional posture.

## Releasing a new version

1. Bump `version` in `Cargo.toml`, `cargo build` to refresh `Cargo.lock`, commit.
2. `git tag -a vX.Y.Z -m "Release vX.Y.Z" && git push origin vX.Y.Z` — this triggers the cargo-dist workflow which publishes the GitHub Release.
3. Bump `pkgver` in both PKGBUILDs (and reset `pkgrel=1`).
4. Run `updpkgsums` in `btop-beautifier-bin/`.
5. In each AUR repo: copy the updated PKGBUILD, regenerate `.SRCINFO`, commit, push.
