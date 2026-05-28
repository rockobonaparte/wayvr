#!/bin/sh
# 1. Hide the system pkg-config tracking paths
export PKG_CONFIG_PATH=""
export PKG_CONFIG_ALLOW_SYSTEM_LIBS=0

# 2. Tell shaderc-sys exactly where your custom build is
export SHADERC_LIB_DIR=/usr/local/lib

# 3. Force the dynamic linker to prioritize /usr/local/lib over system paths
export LD_LIBRARY_PATH=/usr/local/lib:$LD_LIBRARY_PATH

cargo build --release --no-default-features --features=wayland,openxr 2>&1 | tee build.log
