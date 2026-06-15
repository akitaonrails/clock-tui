# typed: true
# frozen_string_literal: true

# Homebrew formula for clock-tui (binary: tclock).
#
# This formula builds from source via Cargo, so it works on any macOS or Linux
# machine with a Rust toolchain available at install time (Homebrew pulls in
# `rust` as a build dependency automatically).
#
# Future work — prebuilt bottles:
#   Once the release workflow publishes macOS tarballs
#   (clock-tui-macos-aarch64.tar.gz / clock-tui-macos-x86_64.tar.gz), this
#   formula can be extended with a `bottle do ... end` block, or switched to a
#   binary install that downloads the matching release asset and verifies its
#   SHA256. Building from source is kept as the baseline so `brew install`
#   works immediately, independent of the release cadence.
class Tclock < Formula
  desc "Terminal clock app with clock, timer, stopwatch, countdown, and widgets"
  homepage "https://github.com/akitaonrails/clock-tui"
  url "https://github.com/akitaonrails/clock-tui/archive/refs/tags/v0.6.8.tar.gz"
  # sha256 of the source tarball above — recompute when bumping the version:
  #   curl -fsSL <url> | shasum -a 256
  sha256 "6b4951e812dc6da420fdb7af7b7c1245d5fd70e4902357ac59969605a2a30525"
  license "MIT"
  head "https://github.com/akitaonrails/clock-tui.git", branch: "master"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", "--root", prefix, "--path", "clock-tui", "--bin", "tclock"
  end

  test do
    assert_match "tclock", shell_output("#{bin}/tclock --help")
  end
end
