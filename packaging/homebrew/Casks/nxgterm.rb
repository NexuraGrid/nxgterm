# Cask for the tap NexuraGrid/homebrew-tap (Casks/nxgterm.rb).
# Update version and sha256 with packaging/update-manifests.sh.
cask "nxgterm" do
  version "0.14.0"
  sha256 "a94ac2da8e9db69bd517fbb95d0fbb2860db8a39d4c0bc72ac2b3584459ecb28"

  url "https://github.com/NexuraGrid/nxgterm/releases/download/v#{version}/nxgterm-#{version}-universal-macos.dmg"
  name "nxgterm"
  desc "Fast, configurable, cross-platform terminal emulator"
  homepage "https://github.com/NexuraGrid/nxgterm"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on :macos

  app "nxgterm.app"
  binary "#{appdir}/nxgterm.app/Contents/MacOS/nxgterm"

  zap trash: "~/.config/nxgterm"

  caveats <<~EOS
    nxgterm is not notarized yet. If macOS says it cannot be opened, run:
      xattr -dr com.apple.quarantine #{appdir}/nxgterm.app

    The optional tools profile (Yazi, zoxide, ngmux, Bruno CLI, curl):
      sh #{appdir}/nxgterm.app/Contents/Resources/profile/install.sh
  EOS
end
