# Fedora / COPR spec. Builds from the release source tarball with cargo.
# COPR: enable "Enable internet access during builds" in the project
# settings, because the crates are fetched from crates.io (they are not
# packaged as Fedora rust-* packages).

# The release profile strips the binary, so there is no debuginfo to split.
%global debug_package %{nil}

Name:           nxgterm
Version:        0.2.0
Release:        1%{?dist}
Summary:        Fast, configurable, cross-platform terminal emulator

License:        MIT OR Apache-2.0
URL:            https://github.com/NexuraGrid/nxgterm
Source0:        %{url}/releases/download/v%{version}/%{name}-%{version}-source.tar.gz

ExclusiveArch:  x86_64 aarch64

BuildRequires:  cargo >= 1.85
BuildRequires:  rust >= 1.85
BuildRequires:  gcc
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

# X11, Wayland and the GPU drivers are loaded at runtime (dlopen), so the
# automatic dependency generator does not see them.
Requires:       hicolor-icon-theme
Requires:       libxkbcommon
Recommends:     libxkbcommon-x11
Recommends:     libwayland-client
Recommends:     libwayland-cursor
Recommends:     libwayland-egl
Recommends:     libX11-xcb
Recommends:     libXcursor
Recommends:     libXi
Recommends:     libXrandr
Recommends:     vulkan-loader
Recommends:     mesa-libEGL

%description
nxgterm is a terminal emulator with a GPU renderer that falls back to the
CPU automatically, TOML configuration with live reload and themes, and
inline images through the Kitty graphics protocol and Sixel. The optional
tools profile installer is shipped in %{_datadir}/%{name}/profile.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked -p nxgterm

%install
install -Dpm 0755 target/release/nxgterm %{buildroot}%{_bindir}/nxgterm
install -Dpm 0644 assets/linux/nxgterm.desktop %{buildroot}%{_datadir}/applications/nxgterm.desktop
install -Dpm 0644 assets/linux/io.github.nexuragrid.nxgterm.metainfo.xml \
    %{buildroot}%{_metainfodir}/io.github.nexuragrid.nxgterm.metainfo.xml
install -Dpm 0644 assets/nxgterm.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/nxgterm.svg
for size in 16 24 32 48 64 128 256 512; do
    install -Dpm 0644 assets/icons/nxgterm-$size.png \
        %{buildroot}%{_datadir}/icons/hicolor/${size}x${size}/apps/nxgterm.png
done
install -Dpm 0755 profile/install.sh %{buildroot}%{_datadir}/%{name}/profile/install.sh
install -Dpm 0644 profile/install.ps1 %{buildroot}%{_datadir}/%{name}/profile/install.ps1

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/nxgterm.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/io.github.nexuragrid.nxgterm.metainfo.xml
cargo test --release --locked --workspace

%files
%license LICENSE-MIT LICENSE-APACHE
%doc README.md
%{_bindir}/nxgterm
%{_datadir}/applications/nxgterm.desktop
%{_metainfodir}/io.github.nexuragrid.nxgterm.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/nxgterm.svg
%{_datadir}/icons/hicolor/*/apps/nxgterm.png
%{_datadir}/%{name}/

%changelog
* Wed Oct 07 2026 NexuraGrid <https://github.com/NexuraGrid> - 0.2.0-1
- Scrollback, mouse wheel and mouse reporting, tabs, command palette,
  configurable key bindings, font fallback for icons, sixel fix for yazi

* Wed Oct 07 2026 NexuraGrid <https://github.com/NexuraGrid> - 0.1.0-1
- Initial package
