#!/usr/bin/env bash
# Builds the reference interpreters the differential fuzz target and tools/lua_diff compare luagee with, from pinned,
# checksummed sources, into temp/ref (outside every Cargo dependency graph). A download whose checksum differs is
# refused. Prints the path of each interpreter built.
#
#   tools/ref/build.sh            build what is missing
#
# temp/ref/luajit/src/luajit   LuaJIT 2.1 (run it with -joff: the interpreter, not the JIT)
# temp/ref/lua-5.1.5/src/lua   PUC-Rio Lua 5.1.5
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
ref=$here/temp/ref
mkdir -p "$ref"
cd "$ref"

luajit_commit=ea9eb7edb5eac4a9d175d74523db34fac0fa5180
luajit_sha256=338a5640be00b89fd0a2a027a7433fcbbb4d006be840d216d84151386b5bcdf7
lua51_sha256=2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333

fetch() { # url file sha256
    if [ ! -f "$2" ]; then
        curl -fsSL -A "Mozilla/5.0" -o "$2.part" "$1"
        mv "$2.part" "$2"
    fi
    if ! echo "$3  $2" | sha256sum -c --quiet -; then
        echo "checksum mismatch: $2 (from $1); refusing it" >&2
        rm -f "$2"
        exit 1
    fi
}

jobs=$(( $(nproc) / 4 > 0 ? $(nproc) / 4 : 1 ))
if [ ! -x luajit/src/luajit ]; then
    fetch "https://github.com/LuaJIT/LuaJIT/archive/$luajit_commit.tar.gz" luajit.tar.gz "$luajit_sha256"
    rm -rf luajit "LuaJIT-$luajit_commit"
    tar xzf luajit.tar.gz
    mv "LuaJIT-$luajit_commit" luajit
    # The archive carries no git metadata: the version string is the commit's.
    echo 1793383622 > luajit/.relver
    make -C luajit -j"$jobs" >/dev/null
fi
if [ ! -x lua-5.1.5/src/lua ]; then
    fetch https://www.lua.org/ftp/lua-5.1.5.tar.gz lua-5.1.5.tar.gz "$lua51_sha256"
    rm -rf lua-5.1.5
    tar xzf lua-5.1.5.tar.gz
    # Plain POSIX: no readline, nothing beyond libc and libm.
    make -C lua-5.1.5/src -j"$jobs" all MYCFLAGS=-DLUA_USE_POSIX MYLIBS= >/dev/null
fi
echo "$ref/luajit/src/luajit"
echo "$ref/lua-5.1.5/src/lua"
