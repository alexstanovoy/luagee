-- expect: error bytecode
local f, message = loadstring("\27LJ\2\0\1\2\3") if f then return "loaded" end error(message)
