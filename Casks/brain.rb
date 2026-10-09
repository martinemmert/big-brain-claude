cask "brain" do
  version "0.7.1"
  sha256 "52580c9617287b6d1038b44fee4a9a1bd60122f781942e041598dfa2bd67c7f6"

  url "https://github.com/martinemmert/big-brain-claude/releases/download/v#{version}/Brain-#{version}-macos-arm64.zip"
  name "Brain"
  desc "Dashboard for Claude Code sessions across accounts"
  homepage "https://github.com/martinemmert/big-brain-claude"

  depends_on arch: :arm64
  depends_on macos: :ventura

  app "Brain.app"
  binary "#{appdir}/Brain.app/Contents/MacOS/brain"

  zap trash: "~/.claude-brain"

  caveats <<~EOS
    Brain is only ad-hoc signed, not notarized, so macOS blocks its first start.
    Allow it in System Settings → Privacy & Security → Open Anyway, or run:
      xattr -dr com.apple.quarantine #{appdir}/Brain.app

    Then run once, to add Brain's hooks to every ~/.claude* account:
      brain install

    Before uninstalling, run `brain uninstall` to remove them again.
  EOS
end
