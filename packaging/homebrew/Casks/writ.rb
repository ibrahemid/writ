cask "writ" do
  version "0.7.0-3"
  sha256 "56f56ec9aaba2fa2f187f557754ce0ffc5da5477eb3087ecbb017991a5c20120"

  url "https://github.com/ibrahemid/writ/releases/download/v#{version}/Writ_#{version}_universal.pkg"

  name "Writ"
  desc "Light text editor for any text file, with full-text search and inline Markdown"
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
