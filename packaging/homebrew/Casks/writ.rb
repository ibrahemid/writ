cask "writ" do
  version "0.6.0-1"
  sha256 "c3ab82d2d98071b195987c42daa5864a0475b79d28bd12b8edac07be8bee9347"

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
