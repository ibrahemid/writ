cask "writ" do
  version "0.3.1"
  sha256 "96bb486e9f5110f5325186de4e515757bd91b30cb7cfed13ab476fd0a08a4c8e"

  url "https://github.com/ibrahemid/writ/releases/download/v#{version}/Writ_#{version}_universal.pkg",
      verified: "github.com/ibrahemid/writ/"

  name "Writ"
  desc "Lightweight, always-ready text editor for developers"
  homepage "https://github.com/ibrahemid/writ"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates true
  depends_on macos: :monterey

  pkg "Writ_#{version}_universal.pkg"

  uninstall pkgutil: "com.writ.editor",
            quit: "com.writ.editor"

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
