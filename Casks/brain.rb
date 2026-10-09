cask "brain" do
  version "0.7.0"
  sha256 "61bd3f0319e8933326732c904f309b383f2ad83a95465581f574e4b3b3bad778"

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
