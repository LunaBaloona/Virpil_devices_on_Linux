//! elite-hotas-fix: keep joysticks and throttles working in Elite Dangerous
//! (or technicallyany Steam game) running under Proton on Linux.
//!
//! The problem it solves: when a Proton version treats a flight controller as an
//! Xbox-style gamepad, Wine records the device in the prefix registry with an
//! "&IG_00" suffix. Those records stay behind after you switch Proton versions,
//! and the game keeps seeing the stick as an old-fashioned XInput pad ("XB360 Pad") instead of
//! a DirectInput joystick, so it's bindings stop working.
//!
//! Our tool:
//!   check  report connected flight controllers, raw-HID access, stale gamepad
//!          entries in the prefix, and the Steam launch options to use
//!   fix    back up system.reg and remove the stale entries
//!   udev   (with sudo) install a udev rule giving the logged-in user raw HID
//!          access to the connected flight controllers
//!
//! Nothing about any particular machine is built in. Flight controllers are
//! recognised from their own HID report descriptors, and the user's Steam libraries are
//! found from Steam's libraryfolders.vdf.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const TOOL: &str = "elite-hotas-fix";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_APPID: u32 = 359320; // Elite Dangerous on Steam
const UDEV_RULE_PATH: &str = "/etc/udev/rules.d/70-elite-hotas.rules";

const HELP: &str = "\
Keep flight sticks and throttles working in Elite Dangerous under Proton.

USAGE
    elite-hotas-fix [COMMAND] [OPTIONS]

COMMANDS
    check   (default) Report devices, raw-HID access, stale gamepad entries in
            the game's Proton prefix, and the Steam launch options to use.
            Changes nothing. If run in a terminal and a fix is needed, it asks
            whether to apply it.
    fix     Back up the prefix registry, then remove stale gamepad entries for
            your flight controllers. Close the game first.
    udev    Install a udev rule so your user can open the controllers' raw HID
            devices. Run with sudo (once per OS install, or after adding a new
            stick)0. Without sudo it only shows the rule.

OPTIONS
    --appid N             Steam app ID (default 359320, Elite Dangerous)
    --prefix PATH         Use this Wine prefix instead of searching for it
                          (the .../compatdata/<appid>/pfx folder)
    --device VID:PID      Treat this device as a flight controller even if it
                          is not detected as one (hex, e.g. 1234:abcd).
                          Repeat for several devices.
    -h, --help            Show this help
    -V, --version         Show the version

WHEN TO USE
    After an OS reinstall:      sudo elite-hotas-fix udev
                                then launch the game once, quit, run 'check'
    After changing Proton:      elite-hotas-fix check   (then 'fix' if asked)
    A stick stopped working:    elite-hotas-fix check

EXIT STATUS
    0 all good / done    1 problems found    2 error
";

// --------------------------------------------------------------------- types

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Id {
    vid: u16,
    pid: u16,
}

impl Id {
    fn parse(s: &str) -> Option<Id> {
        let (a, b) = s.trim().split_once([':', '/'])?;
        Some(Id { vid: hex16(a)?, pid: hex16(b)? })
    }
    fn colon(&self) -> String {
        format!("{:04x}:{:04x}", self.vid, self.pid)
    }
    fn hidraw(&self) -> String {
        format!("0x{:04x}/0x{:04x}", self.vid, self.pid)
    }
}

fn hex16(s: &str) -> Option<u16> {
    let s = s.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    u16::from_str_radix(s, 16).ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Flight,  // HID application collection: Joystick or Multi-axis Controller
    Gamepad, // HID application collection: Game Pad
    Other,   // keyboards, mice, tablets, vendor-specific interfaces...
}

fn merge_kind(a: Kind, b: Kind) -> Kind {
    use Kind::*;
    match (a, b) {
        (Gamepad, _) | (_, Gamepad) => Gamepad,
        (Flight, _) | (_, Flight) => Flight,
        _ => Other,
    }
}

struct Device {
    id: Id,
    name: String,
    kind: Kind,
    nodes: Vec<String>,
}

impl Device {
    /// Can the current user open every raw HID node of this device?
    fn accessible(&self) -> bool {
        self.nodes
            .iter()
            .all(|n| OpenOptions::new().read(true).write(true).open(n).is_ok())
    }
}

struct Opts {
    command: String,
    appid: u32,
    prefix: Option<PathBuf>,
    forced: Vec<Id>,
}

// ------------------------------------------------------------------- devices

fn sys_root() -> PathBuf {
    // Override only used for testing against a fake /sys tree.
    env::var_os("ELITE_HOTAS_FIX_SYSFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys"))
}

/// "0003:00001234:0000ABCD" -> Id
fn parse_hid_id(v: &str) -> Option<Id> {
    let parts: Vec<&str> = v.trim().split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let vid = u32::from_str_radix(parts[1], 16).ok()?;
    let pid = u32::from_str_radix(parts[2], 16).ok()?;
    Some(Id { vid: (vid & 0xffff) as u16, pid: (pid & 0xffff) as u16 })
}

/// Read the top-level application collections of a HID report descriptor.
fn classify(desc: &[u8]) -> Kind {
    let mut i = 0usize;
    let mut usage_page: u32 = 0;
    let mut usages: Vec<u32> = Vec::new();
    let mut depth: i32 = 0;
    let mut kind = Kind::Other;

    while i < desc.len() {
        let b = desc[i];
        if b == 0xFE {
            // long item: 0xFE, size, tag, data...
            let Some(&size) = desc.get(i + 1) else { break };
            i += 3 + size as usize;
            continue;
        }
        let size = match b & 0x03 {
            3 => 4,
            n => n as usize,
        };
        if i + 1 + size > desc.len() {
            break;
        }
        let mut data: u32 = 0;
        for k in 0..size {
            data |= (desc[i + 1 + k] as u32) << (8 * k);
        }
        let typ = (b >> 2) & 0x03;
        let tag = b >> 4;
        match (typ, tag) {
            (1, 0x0) => usage_page = data, // global: Usage Page
            (2, 0x0) => usages.push(if size == 4 { data } else { (usage_page << 16) | data }),
            (0, 0xA) => {
                // main: Collection; 0x01 = Application
                if depth == 0 && data == 0x01 {
                    let this = match usages.first().copied().unwrap_or(0) {
                        0x0001_0004 | 0x0001_0008 => Kind::Flight,
                        0x0001_0005 => Kind::Gamepad,
                        _ => Kind::Other,
                    };
                    kind = merge_kind(kind, this);
                }
                depth += 1;
            }
            (0, 0xC) => depth = (depth - 1).max(0), // End Collection
            _ => {}
        }
        if typ == 0 {
            usages.clear(); // local items end at every main item
        }
        i += 1 + size;
    }
    kind
}

fn scan_devices() -> Vec<Device> {
    let dir = sys_root().join("class/hidraw");
    let mut entries: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort_by_key(|p| {
        // hidraw2 before hidraw10
        p.file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.trim_start_matches("hidraw").parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });

    let mut map: BTreeMap<Id, Device> = BTreeMap::new();
    for e in entries {
        let Some(node_name) = e.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !node_name.starts_with("hidraw") {
            continue;
        }
        let Ok(uevent) = fs::read_to_string(e.join("device/uevent")) else { continue };
        let mut id = None;
        let mut name = String::new();
        for line in uevent.lines() {
            if let Some(v) = line.strip_prefix("HID_ID=") {
                id = parse_hid_id(v);
            } else if let Some(v) = line.strip_prefix("HID_NAME=") {
                name = v.trim().to_owned();
            }
        }
        let Some(id) = id else { continue };
        let kind = fs::read(e.join("device/report_descriptor"))
            .map(|d| classify(&d))
            .unwrap_or(Kind::Other);
        let dev = map.entry(id).or_insert_with(|| Device {
            id,
            name: String::new(),
            kind: Kind::Other,
            nodes: Vec::new(),
        });
        dev.kind = merge_kind(dev.kind, kind);
        if dev.name.is_empty() {
            dev.name = name;
        }
        dev.nodes.push(format!("/dev/{node_name}"));
    }
    map.into_values().collect()
}

/// Devices to treat as flight controllers: detected ones plus --device ones.
fn target_ids(devs: &[Device], forced: &[Id]) -> BTreeSet<Id> {
    devs.iter()
        .filter(|d| d.kind == Kind::Flight)
        .map(|d| d.id)
        .chain(forced.iter().copied())
        .collect()
}

// --------------------------------------------------------------------- steam

fn steam_roots() -> Vec<PathBuf> {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let candidates = [
        ".local/share/Steam",
        ".steam/steam",
        ".steam/root",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam", // Flatpak
        ".var/app/com.valvesoftware.Steam/data/Steam",     // Flatpak (older )
        "snap/steam/common/.local/share/Steam",     // Snap
    ];
    let mut seen = BTreeSet::new();
    candidates
        .iter()
        .filter_map(|c| fs::canonicalize(home.join(c)).ok())
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// Library folders listed in a libraryfolders.vdf file.
fn vdf_paths(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("\"path\"") else { continue };
        let rest = rest.trim();
        if rest.len() >= 2 && rest.starts_with('"') && rest.ends_with('"') {
            out.push(PathBuf::from(rest[1..rest.len() - 1].replace("\\\\", "\\")));
        }
    }
    out
}

fn library_dirs() -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for root in steam_roots() {
        let mut libs = vec![root.clone()];
        for vdf in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
            if let Ok(t) = fs::read_to_string(root.join(vdf)) {
                libs.extend(vdf_paths(&t));
            }
        }
        for l in libs {
            if let Ok(c) = fs::canonicalize(&l) {
                if seen.insert(c.clone()) {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn mtime(p: &Path) -> std::time::SystemTime {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::UNIX_EPOCH)
}

/// All prefixes for the app, most recently used first.
fn find_prefixes(appid: u32) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut out: Vec<PathBuf> = library_dirs()
        .into_iter()
        .map(|l| l.join(format!("steamapps/compatdata/{appid}/pfx")))
        .filter(|p| p.join("system.reg").is_file())
        .filter_map(|p| fs::canonicalize(p).ok())
        .filter(|p| seen.insert(p.clone()))
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(mtime(&p.join("system.reg"))));
    out
}

/// Accept the pfx folder, its parent compatdata/<appid> folder, or system.reg.
fn normalize_prefix(p: &Path) -> Option<PathBuf> {
    let p = fs::canonicalize(p).ok()?;
    if p.is_file() && p.file_name().is_some_and(|n| n == "system.reg") {
        return p.parent().map(Path::to_path_buf);
    }
    if p.join("system.reg").is_file() {
        return Some(p);
    }
    if p.join("pfx/system.reg").is_file() {
        return Some(p.join("pfx"));
    }
    None
}

/// Returns (chosen prefix, other prefixes found).
fn resolve_prefix(o: &Opts) -> Result<(PathBuf, Vec<PathBuf>), String> {
    if let Some(p) = &o.prefix {
        return normalize_prefix(p)
            .map(|p| (p, Vec::new()))
            .ok_or_else(|| format!("no Wine prefix (system.reg) found at {}", p.display()));
    }
    let mut all = find_prefixes(o.appid);
    if all.is_empty() {
        return Err(format!(
            "no Proton prefix found for Steam app {}. Launch the game once with Proton, \
             quit, and run this again (or give the path with --prefix).",
            o.appid
        ));
    }
    let first = all.remove(0);
    Ok((first, all))
}

fn proton_version(pfx: &Path) -> String {
    pfx.parent()
        .and_then(|d| fs::read_to_string(d.join("version")).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

// ------------------------------------------------------------------ registry
struct Reg {
    head: Vec<u8>,
    sections: Vec<Vec<u8>>,
}

fn read_reg(path: &Path) -> io::Result<Reg> {
    let data = fs::read(path)?;
    let mut head = Vec::new();
    let mut sections: Vec<Vec<u8>> = Vec::new();
    for line in data.split_inclusive(|&b| b == b'\n') {
        if line.first() == Some(&b'[') {
            sections.push(line.to_vec());
        } else if let Some(s) = sections.last_mut() {
            s.extend_from_slice(line);
        } else {
            head.extend_from_slice(line);
        }
    }
    Ok(Reg { head, sections })
}

fn first_line(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == b'\n') {
        Some(n) => &b[..n],
        None => b,
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn hex_u16(b: &[u8]) -> Option<u16> {
    u16::from_str_radix(std::str::from_utf8(b).ok()?, 16).ok()
}

/// Find "vid_XXXX&pid_YYYY&ig_" (any case) in a line.
fn ig_id(line: &[u8]) -> Option<Id> {
    let l = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = find(&l[from..], b"vid_") {
        let s = from + pos;
        if l.len() >= s + 21 && &l[s + 8..s + 13] == b"&pid_" && &l[s + 17..s + 21] == b"&ig_" {
            if let (Some(vid), Some(pid)) = (hex_u16(&l[s + 4..s + 8]), hex_u16(&l[s + 13..s + 17])) {
                return Some(Id { vid, pid });
            }
        }
        from = s + 4;
    }
    None
}

fn stale_counts(reg: &Reg) -> BTreeMap<Id, usize> {
    let mut m = BTreeMap::new();
    for s in &reg.sections {
        if let Some(id) = ig_id(first_line(s)) {
            *m.entry(id).or_insert(0) += 1;
        }
    }
    m
}

/// v0.9.1: Returns the new file contents and how many keys were removed per device.
fn strip_stale(reg: &Reg, targets: &BTreeSet<Id>) -> (Vec<u8>, BTreeMap<Id, usize>) {
    let mut out = reg.head.clone();
    let mut removed = BTreeMap::new();
    for s in &reg.sections {
        match ig_id(first_line(s)) {
            Some(id) if targets.contains(&id) => *removed.entry(id).or_insert(0) += 1,
            _ => out.extend_from_slice(s),
        }
    }
    (out, removed)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{TOOL}.tmp"));
    let perms = fs::metadata(path)?.permissions();
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::set_permissions(&tmp, perms)?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

// ------------------------------------------------------------------- system

fn effective_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

fn timestamp() -> String {
    Command::new("date")
        .arg("+%Y%m%d-%H%M%S")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            secs.to_string()
        })
}

fn same_path(a: &str, b: &Path) -> bool {
    let a = a.trim_end_matches('/');
    if a.is_empty() {
        return false;
    }
    match fs::canonicalize(a) {
        Ok(c) => c == b,
        Err(_) => Path::new(a) == b,
    }
}

/// PIDs of processes (game, wineserver, Proton) using this prefix.
fn prefix_users(pfx: &Path) -> Vec<u32> {
    let compat = pfx.parent().map(Path::to_path_buf).unwrap_or_default();
    let me = std::process::id();
    let mut pids = Vec::new();
    let Ok(rd) = fs::read_dir("/proc") else { return pids };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(env) = fs::read(e.path().join("environ")) else { continue };
        for var in env.split(|&b| b == 0) {
            let var = String::from_utf8_lossy(var);
            let hit = if let Some(v) = var.strip_prefix("WINEPREFIX=") {
                same_path(v, pfx)
            } else if let Some(v) = var.strip_prefix("STEAM_COMPAT_DATA_PATH=") {
                same_path(v, &compat)
            } else {
                false
            };
            if hit {
                pids.push(pid);
                break;
            }
        }
    }
    pids
}

fn ask_yes_no(question: &str) -> bool {
    print!("{question} [y/N] ");
    let _ = io::stdout().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn exe_name() -> String {
    env::args().next().unwrap_or_else(|| TOOL.into())
}

// ----------------------------------------------------------------- commands

fn print_devices(devs: &[Device], forced: &[Id]) -> bool {
    let mut ok = true;
    println!("Flight controllers:");
    let mut any = false;
    for d in devs.iter().filter(|d| d.kind == Kind::Flight || forced.contains(&d.id)) {
        any = true;
        let access = d.accessible();
        ok &= access;
        println!(
            "   {}  {:<9}  {}  ({})",
            if access { "[ok]       " } else { "[NO ACCESS]" },
            d.id.colon(),
            d.name,
            d.nodes.join(", ")
        );
    }
    for f in forced.iter().filter(|f| !devs.iter().any(|d| d.id == **f)) {
        any = true;
        println!("   [not connected]  {}  (given with --device)", f.colon());
    }
    if !any {
        println!("   none detected. Plug them in. If a stick shows up under 'Gamepads'");
        println!("   below, name it with --device VID:PID.");
    }
    let pads: Vec<&Device> = devs
        .iter()
        .filter(|d| d.kind == Kind::Gamepad && !forced.contains(&d.id))
        .collect();
    if !pads.is_empty() {
        println!("\nGamepads (left alone):");
        for d in pads {
            println!("   {:<9}  {}", d.id.colon(), d.name);
        }
    }
    let others = devs
        .iter()
        .filter(|d| d.kind == Kind::Other && !forced.contains(&d.id))
        .count();
    if others > 0 {
        println!("\n({others} other HID device(s) ignored: keyboards, mice, etc.)");
    }
    if !ok {
        println!("\n   -> no raw HID access. Fix with:  sudo {} udev", exe_name());
    }
    ok
}

fn print_launch_options(targets: &BTreeSet<Id>) {
    if targets.is_empty() {
        return;
    }
    let list: Vec<String> = targets.iter().map(Id::hidraw).collect();
    println!("\nSteam launch options (Properties > General), keep any game arguments");
    println!("you already use after %command%:");
    println!("   PROTON_ENABLE_HIDRAW={} SDL_JOYSTICK_HIDAPI=0 %command%", list.join(","));
}

/// 0.1.2 this time explain why stale entries for non-target devices are being left alone.
fn print_left_alone(stale: &BTreeMap<Id, usize>, targets: &BTreeSet<Id>, devs: &[Device]) {
    let others: Vec<(&Id, &usize)> = stale.iter().filter(|(id, _)| !targets.contains(id)).collect();
    if others.is_empty() {
        return;
    }
    println!("\nOther gamepad (IG_) entries:");
    for (id, n) in others {
        let why = match devs.iter().find(|d| d.id == *id).map(|d| d.kind) {
            Some(Kind::Gamepad) => "a real gamepad, entry is expected",
            Some(_) => "connected, but not a joystick",
            None => "not connected; plug it in or use --device if it is a stick",
        };
        println!("   {}  {n} key(s), left alone ({why})", id.colon());
    }
}

fn cmd_check(o: &Opts) -> u8 {
    println!("{TOOL} {VERSION}\n");
    let devs = scan_devices();
    let targets = target_ids(&devs, &o.forced);
    let mut problems = !print_devices(&devs, &o.forced);

    println!();
    let (pfx, others) = match resolve_prefix(o) {
        Ok(v) => v,
        Err(e) => {
            println!("Prefix: {e}");
            print_launch_options(&targets);
            println!("\nResult: prefix not checked");
            return 1;
        }
    };
    println!("Prefix:        {}", pfx.display());
    println!("Proton (last): {}", proton_version(&pfx));
    for p in &others {
        println!("Also found:    {}  (older; pick one with --prefix)", p.display());
    }

    let reg = match read_reg(&pfx.join("system.reg")) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", pfx.join("system.reg").display());
            return 2;
        }
    };
    let stale = stale_counts(&reg);
    let to_fix: BTreeMap<Id, usize> = stale
        .iter()
        .filter(|(id, _)| targets.contains(id))
        .map(|(k, v)| (*k, *v))
        .collect();

    println!("\nStale gamepad (IG_) entries for flight controllers:");
    if to_fix.is_empty() {
        println!("   none");
    } else {
        problems = true;
        for (id, n) in &to_fix {
            println!("   {}  {n} key(s)  <- makes the game see it as an Xbox pad", id.colon());
        }
    }
    print_left_alone(&stale, &targets, &devs);
    print_launch_options(&targets);

    println!("\nResult: {}", if problems { "problems found" } else { "all good" });

    if !to_fix.is_empty() {
        if io::stdin().is_terminal() && io::stdout().is_terminal() {
            println!();
            if ask_yes_no("Remove the stale entries now (a backup is made first)?") {
                println!();
                let rc = do_fix(&pfx, &devs, &targets);
                return if rc == 0 && print_access_ok(&devs, &o.forced) { 0 } else { rc.max(1) };
            }
        } else {
            println!("   -> remove with:  {} fix", exe_name());
        }
    }
    u8::from(problems)
}

fn print_access_ok(devs: &[Device], forced: &[Id]) -> bool {
    devs.iter()
        .filter(|d| d.kind == Kind::Flight || forced.contains(&d.id))
        .all(Device::accessible)
}

fn do_fix(pfx: &Path, devs: &[Device], targets: &BTreeSet<Id>) -> u8 {
    let path = pfx.join("system.reg");
    let reg = match read_reg(&path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            return 2;
        }
    };
    if !reg.head.starts_with(b"WINE REGISTRY Version 2") {
        eprintln!("error: {} does not look like a Wine registry file; not touching it.", path.display());
        return 2;
    }

    let stale = stale_counts(&reg);
    if !stale.keys().any(|id| targets.contains(id)) {
        println!("Nothing to fix: no stale gamepad entries for flight controllers.");
        print_left_alone(&stale, targets, devs);
        return 0;
    }

    let users = prefix_users(pfx);
    if !users.is_empty() {
        eprintln!(
            "error: the game (or Wine) is still using this prefix (PIDs {:?}).\n\
             Quit the game, wait a few seconds, and try again.",
            users
        );
        return 2;
    }

    let backup = pfx.join(format!("system.reg.bak-{}", timestamp()));
    if let Err(e) = fs::copy(&path, &backup) {
        eprintln!("error: could not create backup {}: {e}", backup.display());
        return 2;
    }

    let (new, removed) = strip_stale(&reg, targets);
    if let Err(e) = write_atomic(&path, &new) {
        eprintln!("error: could not write {}: {e}\nThe original is unchanged.", path.display());
        return 2;
    }

    println!("Backup:  {}", backup.display());
    for (id, n) in &removed {
        println!("Removed: {}  {n} key(s)", id.colon());
    }

    // Values elsewhere that still point at the removed IDs are harmless, but say so.
    let leftover = new
        .split(|&b| b == b'\n')
        .filter(|l| ig_id(l).is_some_and(|id| targets.contains(&id)))
        .count();
    if leftover > 0 {
        println!("Note:    {leftover} other line(s) still mention these IDs (values, not keys).");
    }
    println!(
        "\nDone. To undo:  cp '{}' '{}'",
        backup.display(),
        path.display()
    );
    println!("Start the game; the stick should be listed under its real name again.");
    0
}

fn cmd_fix(o: &Opts) -> u8 {
    let (pfx, _) = match resolve_prefix(o) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };
    let devs = scan_devices();
    let targets = target_ids(&devs, &o.forced);
    if targets.is_empty() {
        eprintln!(
            "error: no flight controllers detected. Plug them in, or name them with \
             --device VID:PID."
        );
        return 2;
    }
    println!("Prefix:  {}", pfx.display());
    do_fix(&pfx, &devs, &targets)
}

/// (vid, pid) pairs already present in an existing rules file, with their lines.
fn existing_rules(text: &str) -> Vec<(Id, String)> {
    let grab = |line: &str, key: &str| -> Option<u16> {
        let start = line.find(key)? + key.len();
        hex16(line[start..].split('"').next()?)
    };
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let vid = grab(l, "ATTRS{idVendor}==\"")?;
            let pid = grab(l, "ATTRS{idProduct}==\"")?;
            Some((Id { vid, pid }, l.to_owned()))
        })
        .collect()
}

fn rule_line(id: Id) -> String {
    format!(
        "SUBSYSTEM==\"hidraw\", ATTRS{{idVendor}}==\"{:04x}\", ATTRS{{idProduct}}==\"{:04x}\", \
         MODE=\"0660\", TAG+=\"uaccess\"",
        id.vid, id.pid
    )
}

fn cmd_udev(o: &Opts) -> u8 {
    let devs = scan_devices();
    let targets = target_ids(&devs, &o.forced);
    let previous = fs::read_to_string(UDEV_RULE_PATH)
        .map(|t| existing_rules(&t))
        .unwrap_or_default();

    if targets.is_empty() && previous.is_empty() {
        eprintln!(
            "error: no flight controllers detected. Plug them in, or name them with \
             --device VID:PID."
        );
        return 2;
    }

    let mut text = String::from(
        "# Written in by elite-hotas-fix: lets the logged-in user open these flight\n\
         # controllers' raw HID devices (needed for PROTON_ENABLE_HIDRAW).\n\
         # The file number must stay below 73 for TAG+=\"uaccess\" to take effect.\n",
    );
    for id in &targets {
        let name = devs
            .iter()
            .find(|d| d.id == *id)
            .map(|d| d.name.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("(given with --device)");
        text.push_str(&format!("\n# {name}\n{}\n", rule_line(*id)));
    }
    for (id, line) in previous.iter().filter(|(id, _)| !targets.contains(id)) {
        text.push_str(&format!("\n# {} (kept from earlier run; not connected now)\n{line}\n", id.colon()));
    }

    if effective_uid() != Some(0) {
        println!("Run with sudo to install {UDEV_RULE_PATH}:\n");
        println!("   sudo {} udev\n", exe_name());
        println!("It would contain:\n");
        print!("{text}");
        return 1;
    }

    let path = Path::new(UDEV_RULE_PATH);
    let write = (|| {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, &text)
    })();
    if let Err(e) = write {
        eprintln!("error: could not write {UDEV_RULE_PATH}: {e}");
        return 2;
    }
    println!("Wrote {UDEV_RULE_PATH}:\n");
    print!("{text}");

    let reload = [
        vec!["control", "--reload-rules"],
        vec!["trigger", "--subsystem-match=hidraw", "--action=change"],
    ];
    let mut reloaded = true;
    for args in reload {
        match Command::new("udevadm").args(&args).status() {
            Ok(s) if s.success() => {}
            _ => reloaded = false,
        }
    }
    if reloaded {
        println!("\nRules reloaded. If access is still missing, unplug and replug the");
        println!("devices, or log out and back in.");
    } else {
        println!("\nCould not run udevadm; reboot (or replug the devices) to apply the rule.");
    }
    0
}

// --------------------------------------------------------------------- main

fn parse_args() -> Result<Opts, String> {
    let mut o = Opts {
        command: "check".into(),
        appid: DEFAULT_APPID,
        prefix: None,
        forced: Vec::new(),
    };
    let mut command_set = false;
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_owned(), Some(v.to_owned())),
            _ => (a.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match key.as_str() {
            "-h" | "--help" | "help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("{TOOL} {VERSION}");
                std::process::exit(0);
            }
            "--appid" => {
                let v = value("--appid")?;
                o.appid = v.parse().map_err(|_| format!("--appid: '{v}' is not a number"))?;
            }
            "--prefix" => o.prefix = Some(PathBuf::from(value("--prefix")?)),
            "--device" => {
                let v = value("--device")?;
                for part in v.split(',').filter(|p| !p.trim().is_empty()) {
                    let id = Id::parse(part)
                        .ok_or_else(|| format!("--device: '{part}' is not VID:PID in hex (e.g. 1234:abcd)"))?;
                    if !o.forced.contains(&id) {
                        o.forced.push(id);
                    }
                }
            }
            "check" | "fix" | "udev" if !command_set => {
                o.command = key;
                command_set = true;
            }
            other => return Err(format!("unknown argument '{other}' (see --help)")),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    let o = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if o.command != "udev" && effective_uid() == Some(0) {
        eprintln!(
            "error: run '{}' as your normal user, without sudo (root would look in the \
             wrong home folder and change file ownership in the prefix).",
            o.command
        );
        return ExitCode::from(2);
    }
    let rc = match o.command.as_str() {
        "fix" => cmd_fix(&o),
        "udev" => cmd_udev(&o),
        _ => cmd_check(&o),
    };
    ExitCode::from(rc)
}
