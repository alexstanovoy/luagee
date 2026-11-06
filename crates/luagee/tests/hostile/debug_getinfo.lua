-- expect: ok table	debug_getinfo.lua
local info = debug.getinfo(1) print(type(info), info.short_src)
