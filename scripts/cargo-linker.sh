#!/bin/sh
# Cargo linker for aarch64-apple-darwin (see .cargo/config.toml).
#
# Apple's ld-27037.1 (Xcode 27 and its Command Line Tools, Sept 2026) writes
# dylibs with chained fixups whose LINKEDIT string pool is mis-aligned. dyld
# refuses to load them, so every newly built proc-macro fails with
# "can't find crate for `<name>`". Linking .dylib outputs without chained
# fixups avoids it. Proc-macros only ever load inside rustc and never ship;
# the JARVIS executable is not a .dylib and is linked exactly as before.
out=""
prev=""
for arg in "$@"; do
  [ "$prev" = "-o" ] && out="$arg"
  prev="$arg"
done
case "$out" in
  *.dylib) exec cc "$@" -Wl,-no_fixup_chains ;;
  *) exec cc "$@" ;;
esac
