#!/usr/bin/env lua
-- A script whose very first line is a shebang line runs, and the line count stays right.
print("shebang skipped")
print(debug == nil or debug.traceback ~= nil)
local ok, message = pcall(error, "x", 0)
print(ok, message)
