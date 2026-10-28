# Elite-hotas-fix

This is an automation of the steps described in [Virpil_devices_on_Linux](https://github.com/LunaBaloona/Virpil_devices_on_Linux/blob/main/Linux_config_and_gaming.md). Elite-hotas-fix keeps Virpil joysticks and throttles working in Elite Dangerous (or any Steam game) running under Proton on Linux.
It's a single static binary that runs on any x86_64 distro: Arch, Debian, Fedora and others.


## The problem it fixes

Sometimes, after a game update or if you change your Proton version in Steam, a stick or throttle stops responding in-game. Elite Dangerous sees Virpil throttles as **"XB360 Pad"**, and `BindingLoadingErrors.log` says `Failed to find GUID for device`.

This happens because at some point Proton treated the device as an Xbox-style gamepad. Wine then saved it in the game's prefix registry with an `&IG_00` suffix, and that record stays behind after you change Proton version. The game keeps reading the stale entry, so it loads the device as an XInput pad instead of a joystick.

The tool finds those entries for your flight controllers and removes them, after making a backup. It also checks the two other things that commonly go wrong:
- **Raw HID permissions.** It can install a udev rule to fix these.
- **Steam launch options.** It prints the `PROTON_ENABLE_HIDRAW` line for your devices.

## How it decides what is a flight controller

The tool reads each connected device's HID report descriptor. That's the device's own description of itself.
- **Flight controllers.** Devices that describe themselves as a *Joystick* or *Multi-axis Controller* are treated as flight controllers.
- **Gamepads.** Devices that describe themselves as a *Game Pad*, such as a real Xbox controller, are left alone.
- **Everything else.** Keyboards, mice and tablets are ignored.

No device or vendor IDs are built into the tool. If a stick is not detected, name it yourself with `--device VID:PID`. (Run `lsusb` to find the ID; it is the `xxxx:yyyy` pair.)

The tool finds Steam libraries through Steam's `libraryfolders.vdf`, including libraries on other drives, and Flatpak and Snap installs. If it finds several prefixes, it uses the most recently used one.

## Usage

```
elite-hotas-fix              # same as 'check'
elite-hotas-fix check        # report only; asks before fixing if run in a terminal
elite-hotas-fix fix          # back up system.reg and remove stale entries (close the game first)
sudo elite-hotas-fix udev    # install /etc/udev/rules.d/70-elite-hotas.rules
```

Options:

| Option | Meaning |
|---|---|
| `--appid N` | Steam app ID; the default is 359320 (Elite Dangerous) |
| `--prefix PATH` | Use this prefix, the `.../compatdata/<appid>/pfx` folder |
| `--device VID:PID` | Treat this device as a flight controller; repeat for several devices |

Exit status: 0 all good, 1 problems found, 2 error.

### After a fresh OS install

1. Copy `elite-hotas-fix` somewhere handy, e.g. `~/.local/bin/`, and make it executable: `chmod +x elite-hotas-fix`.
2. Plug in all your sticks, then run `sudo elite-hotas-fix udev`. Unplug and replug the devices, or log out and back in.
3. Install the game, choose your Proton version, launch the game once and quit.
4. Run `elite-hotas-fix`. Paste the launch options it prints into Steam (game > Properties > General), then answer **y** if it offers a fix.

### After changing Proton version

Run `elite-hotas-fix` with the game closed. If it reports stale entries, let it fix them.

### Undo

`fix` prints the exact `cp` command that restores the backup. Backups are named `system.reg.bak-YYYYmmdd-HHMMSS` and sit next to `system.reg`.

## Safety

- `check` never changes anything.
- `fix` refuses to run while the game or Wine is still using the prefix.
- `fix` always makes a backup first, and writes the new file atomically. If the write fails, the original is left untouched.
- `fix` removes only registry keys whose name contains `VID_xxxx&PID_yyyy&IG_` for devices it has identified as flight controllers. Every other byte of the file is written back unchanged.
- `check` and `fix` refuse to run as root, so files in the prefix keep the right owner. Only `udev` needs sudo.

## Building from source

You need Rust (`rustup`, or your distro's `rust` package). No crates are downloaded.

```
cd elite-hotas-fix
cargo build --release
# binary: target/release/elite-hotas-fix
```

`.cargo/config.toml` builds a fully static binary. That needs your distro's static C library:
- Arch: included in `glibc`
- Debian/Ubuntu: `libc6-dev`
- Fedora: `glibc-static`

If that library is not available, delete `.cargo/config.toml` for a normal build.

Release builds use musl instead, so they don't depend on the glibc or kernel version of the build machine (Arch: `pacman -S rust-musl`; with rustup: `rustup target add x86_64-unknown-linux-musl`):

```
cargo build --release --target x86_64-unknown-linux-musl
# binary: target/x86_64-unknown-linux-musl/release/elite-hotas-fix
```

To build the release tarball and all three packages for a tagged version, run `packaging/build-packages.sh`. It needs `makepkg`, `dpkg-deb` and `rpmbuild` (on Arch: `pacman -S dpkg rpm-tools`) and writes everything, with `SHA256SUMS`, to `dist/`.
