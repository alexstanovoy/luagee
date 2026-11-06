-- %a and %A as LuaJIT writes them: subnormals normalized, rounding half up to the precision (a carry makes the
-- leading digit 2), the flags and widths of the other conversions.
local cases = {
  {"%a", 1}, {"%a", -1}, {"%a", 0}, {"%a", -0.0}, {"%a", 0.5}, {"%a", 1/3}, {"%a", 2^-1074}, {"%a", 2^-1030 * 3},
  {"%a", 2^-1022}, {"%a", (2^53-1)*2^971}, {"%.0a", (2^53-1)*2^971}, {"%.0a", 1.5}, {"%.0a", 2.5 / 2}, {"%.0a", 1.5 * 2^-1074 * 2^52},
  {"%.1a", 0x1.08p0}, {"%.1a", 0x1.18p0}, {"%.1a", 0x1.081p0}, {"%.1a", 0x1.f8p0}, {"%.3a", 1}, {"%.20a", 1},
  {"%A", 255.5}, {"%#a", 1}, {"%#.0a", 1}, {"%+a", 1}, {"% a", 1}, {"%010a", 1.5}, {"%-10a|", 1.5}, {"%a", 1/0}, {"%a", -1/0},
  {"%a", 0/0}, {"%A", 1/0}, {"%10a", 1/0}, {"%010a", -1/0}, {"%.2a", 2^-1074}, {"%.0a", 2^-1074}, {"%a", 2^-1073 + 2^-1074},
  {"%.13a", 3.14159265358}, {"%.14a", 3.14159265358}, {"%.12a", 0x1.fffffffffffff8p0}, {"%.0a", 0x1.8p0}, {"%.0a", 0x1.7p0},
}
for _, c in ipairs(cases) do print(c[1], string.format(c[1], c[2])) end
for p = 0, 13 do
  local m = 2 - 2^(-4 * p - 1) -- 0x1.ff...f8: p f digits, then 8
  print(p, string.format("%." .. p .. "a", m), string.format("%." .. p .. "a", m * 2^-1022), string.format("%." .. p .. "a", m * 2^1000))
end
print(string.format("%.0a", 1.5 * 2^-1022), string.format("%.0a", 1.5 * 2^-1021), string.format("%.0a", 1.5 * 2^-1000), string.format("%.0a", 1.5 * 2^-1023))
print(string.format("%.1a", 0x1.f8p-1022), string.format("%.1a", 0x1.f8p-1021))
print(string.format("%.1a", 0x1.f8p-1074 * 2^52), string.format("%.1a", 0x1.f7p0), string.format("%.2a", 0x1.ff8p0))
print(string.format("%.1a", 2^-1074 * 3), string.format("%.0a", 2^-1074 * 3), string.format("%.0a", 2^-1074 * 5), string.format("%.1a", 2^-1074 * 0x1f))
