#!/bin/sh
# Writes the Homebrew cask for the tap repository tanganyika/homebrew-wireant (Casks/wireant.rb):
#   packaging/homebrew/cask.sh <version> <sha256 of the .dmg> > Casks/wireant.rb
# Users: brew install --cask tanganyika/wireant/wireant
set -eu
VER=$1
SHA=$2
cat <<RB
cask "wireant" do
  version "$VER"
  sha256 "$SHA"

  url "https://github.com/tanganyika/wireant/releases/download/v#{version}/wireant-#{version}-macos-universal.dmg"
  name "wireant"
  desc "Wire antenna modelling with MININEC, wiremom and NEC-2 engines"
  homepage "https://github.com/tanganyika/wireant"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :big_sur"

  app "wireant.app"

  # the app is signed ad hoc, not notarised: without this macOS refuses to open it
  postflight do
    system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "#{appdir}/wireant.app"]
  end

  zap trash: "~/Library/Application Support/wireant"
end
RB
