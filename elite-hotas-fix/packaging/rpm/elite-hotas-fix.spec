# Packages the static musl binary from the release tarball, so Fedora and
# openSUSE need no Rust toolchain. Built by ../build-packages.sh.

%global debug_package %{nil}
# Ship the release binary byte for byte: no re-stripping, no build-id links,
# and no automatic library requirements (it is statically linked).
%global __os_install_post %{nil}
%global _build_id_links none

Name:           elite-hotas-fix
Version:        1.0.0
Release:        1
Summary:        Keep flight sticks and throttles working in Proton games
License:        GPL-3.0-or-later
URL:            https://github.com/LunaBaloona/Virpil_devices_on_Linux
Source0:        %{name}-%{version}-x86_64-linux.tar.gz
ExclusiveArch:  x86_64
AutoReqProv:    no

%description
Removes the stale "&IG_00" gamepad entries that Proton leaves in a game's
Wine registry, which make Elite Dangerous see a flight stick or throttle as
an "XB360 Pad". Also checks raw HID access, can install a udev rule, and
prints the PROTON_ENABLE_HIDRAW launch options for your devices.

%prep
%setup -q -n %{name}-%{version}-x86_64-linux

%install
install -Dm755 %{name} %{buildroot}%{_bindir}/%{name}

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}

%changelog
* Wed Oct 28 2026 luna <https://github.com/LunaBaloona/Virpil_devices_on_Linux> - 1.0.0-1
- First release
