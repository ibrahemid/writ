cask "writ" do
  version "0.1.0"

  on_arm do
    sha256 "__SHA256_ARM64__"
    url "https://github.com/ibrahemid/writ/releases/download/v#{version}/Writ_#{version}_aarch64.dmg",
        verified: "github.com/ibrahemid/writ/"
  end

  on_intel do
    sha256 "__SHA256_INTEL__"
    url "https://github.com/ibrahemid/writ/releases/download/v#{version}/Writ_#{version}_x64.dmg",
        verified: "github.com/ibrahemid/writ/"
  end

  name "Writ"
  desc "Lightweight, always-ready text editor for developers"
  homepage "https://github.com/ibrahemid/writ"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: ">= :big_sur"

  app "Writ.app"

  uninstall quit: "com.writ.editor"

  zap trash: [
    "~/Library/Application Support/com.writ.editor",
    "~/Library/Application Support/writ",
    "~/Library/Caches/com.writ.editor",
    "~/Library/Caches/writ",
    "~/Library/Preferences/com.writ.editor.plist",
    "~/Library/Saved Application State/com.writ.editor.savedState",
    "~/Library/WebKit/com.writ.editor",
    "~/Library/Logs/writ",
  ]
end
