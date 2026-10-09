cask "brain" do
  version "0.9.0"
  sha256 "f08597ace71f2dde440d48b3a42030cd01d0557854661adc53b5f707c50fbb85"

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
