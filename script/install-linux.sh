#!/usr/bin/env bash
# The system packages the GPUI platform crate links against. The tray crate
# itself only needs a C compiler; the example pulls in the platform crate.
set -euo pipefail

sudo apt update
# Test on Ubuntu 24.04
sudo apt install -y \
  gcc g++ clang libfontconfig-dev libwayland-dev \
  libxkbcommon-x11-dev libx11-xcb-dev \
  libssl-dev libzstd-dev libasound2-dev \
  vulkan-validationlayers libvulkan1
