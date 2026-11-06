-- LuaJIT bit library: tobit tohex bnot band bor bxor lshift rshift arshift rol ror bswap on negatives and >32-bit inputs.
local bit = bit
print(bit.tobit(0), bit.tobit(1), bit.tobit(-1), bit.tobit(0xffffffff), bit.tobit(0x80000000), bit.tobit(0x7fffffff), bit.tobit(2^32), bit.tobit(2^32 + 5))
print(bit.tobit(2^53), bit.tobit(-2^31), bit.tobit(-2^31 - 1), bit.tobit(2^31), bit.tobit(1.5), bit.tobit(-1.5), bit.tobit(3.99), bit.tobit(0x123456789))
print(bit.tobit("10"), bit.tobit("0x10"), bit.tobit(" 7 "), pcall(bit.tobit, "x") == false, pcall(bit.tobit) == false, pcall(bit.tobit, {}) == false)
print(bit.bnot(0), bit.bnot(-1), bit.bnot(0xffffffff), bit.bnot(0x7fffffff), bit.bnot(1), bit.bnot(2^32))
print(bit.band(0xff, 0x0f), bit.band(-1, 0xff), bit.band(0xf0f0, 0xff00), bit.band(7), bit.band(7, 3, 1), bit.band(-1, -1), bit.band(2^32 + 5, 7))
print(bit.bor(1, 2), bit.bor(0), bit.bor(1, 2, 4, 8), bit.bor(-1, 0), bit.bor(0x80000000, 1), bit.bor(2^32, 1))
print(bit.bxor(5, 3), bit.bxor(-1, 0xff), bit.bxor(7), bit.bxor(1, 2, 3), bit.bxor(0xffffffff, 0xffffffff), bit.bxor(2^31, 2^31))
print(bit.lshift(1, 0), bit.lshift(1, 4), bit.lshift(1, 31), bit.lshift(1, 32), bit.lshift(1, 33), bit.lshift(-1, 1), bit.lshift(0xff, 24), bit.lshift(1, -1), bit.lshift(3, 63))
print(bit.rshift(256, 4), bit.rshift(-1, 1), bit.rshift(-1, 31), bit.rshift(-1, 32), bit.rshift(0x80000000, 31), bit.rshift(1, 1), bit.rshift(-256, 4), bit.rshift(5, -1))
print(bit.arshift(256, 4), bit.arshift(-256, 4), bit.arshift(-1, 31), bit.arshift(-1, 32), bit.arshift(0x7fffffff, 30), bit.arshift(0x80000000, 31), bit.arshift(-8, 1), bit.arshift(-8, 0))
print(bit.rol(1, 1), bit.rol(0x80000000, 1), bit.rol(1, 32), bit.rol(1, 31), bit.rol(0x12345678, 8), bit.rol(1, -1), bit.rol(-1, 5))
print(bit.ror(1, 1), bit.ror(2, 1), bit.ror(1, 32), bit.ror(0x12345678, 8), bit.ror(1, -1), bit.ror(0x80000000, 31), bit.ror(-2, 1))
print(bit.bswap(0x12345678), bit.bswap(1), bit.bswap(-1), bit.bswap(0), bit.bswap(0x78563412), bit.bswap(0xff))
print(bit.tohex(255), bit.tohex(-1), bit.tohex(0), bit.tohex(0x12345678), bit.tohex(255, 4), bit.tohex(255, 2), bit.tohex(255, 1), bit.tohex(0x1234, 2), bit.tohex(255, 0) == "")
print(bit.tohex(255, -4), bit.tohex(-1, -8), bit.tohex(0xabcdef, -4), bit.tohex(255, -2), bit.tohex(1, 8), bit.tohex(1, 9), bit.tohex(1, 100), bit.tohex(1, -9))
print(bit.tohex(0x80000000), bit.tohex(2^32 + 1), bit.tohex(-2^31), bit.tohex(1.9), bit.tohex("255"), pcall(bit.tohex) == false, pcall(bit.tohex, 1, "x") == false)
print(bit.band(0xdeadbeef, 0xffff), bit.rshift(0xdeadbeef, 16), bit.bxor(0xdeadbeef, 0xcafebabe), bit.bor(bit.lshift(0xde, 24), 0xadbeef))
print(bit.band(bit.rshift(0xabcd, 8), 0xff), bit.band(0xabcd, 0xff), bit.bor(bit.lshift(0xab, 8), 0xcd) == 0xabcd)
local flags = 0
flags = bit.bor(flags, 1) flags = bit.bor(flags, 4) flags = bit.bor(flags, 16)
print(flags, bit.band(flags, 4) ~= 0, bit.band(flags, 2) ~= 0, bit.band(flags, bit.bnot(4)))
local h = 5381
for c in ("hash me"):gmatch(".") do h = bit.tobit(h * 33 + c:byte()) end
print(h, bit.tohex(h))
local x = 123456789
for i = 1, 5 do x = bit.bxor(x, bit.lshift(x, 13)) x = bit.bxor(x, bit.rshift(x, 17)) x = bit.bxor(x, bit.lshift(x, 5)) end
print(x, bit.tohex(x), bit.band(x, 0xffff))
print(bit.band(1.9, 3.9), bit.bor(0.5, 0.5), bit.lshift(1.9, 1.9), bit.tobit(-0.5), bit.tobit(0/0), bit.tobit(1/0) == bit.tobit(1/0))
print(bit.band("7", "3"), bit.bor("1", 2), pcall(bit.band, {}, 1) == false, pcall(bit.band, nil, 1) == false, pcall(bit.lshift, 1) == false)
print(type(bit.tobit(1)), bit.tobit(1) == 1, bit.band(0xffffffff, 0xffffffff) == -1)
print(bit.arshift(2^31, 31), bit.lshift(0.5, 3), bit.rshift(2^32 + 2, 1), bit.rol(2^32 + 1, 1))
