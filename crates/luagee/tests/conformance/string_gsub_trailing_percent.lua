-- A `%` at the end of a gsub replacement adds a NUL byte, as LuaJIT reads C's string terminator there.
print(string.byte((string.gsub("a b", " ", "%")), 1, -1))
print(string.byte((string.gsub("a b", " ", "x%")), 1, -1))
print((string.gsub("a b", " ", "%%")))
print((string.gsub("a b", " ", "%x")))
print(pcall(string.gsub, "a b", " ", "%2"))
