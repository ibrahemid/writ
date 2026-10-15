cask "writ" do
  version "0.7.0"
  sha256 "e06fe45a300a6b505b698f6da9488ac3840544f4be13a86e2bc4b23cae10cdfa"

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
