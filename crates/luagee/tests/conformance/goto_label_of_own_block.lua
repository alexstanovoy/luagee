-- A goto jumps to the label of the innermost block that has one, even when that label comes later and an
-- enclosing block's label of the same name came earlier.
local trace = {}
do
  ::a::
  trace[#trace + 1] = "outer"
  if #trace > 3 then error("jumped back to the enclosing label") end
  do
    goto a
    trace[#trace + 1] = "skipped"
    ::a::
  end
  trace[#trace + 1] = "after"
end
print(table.concat(trace, " "))

-- The same from a block nested deeper than the label's.
local n = 0
do
  ::b::
  n = n + 1
  if n > 3 then error("jumped back to the enclosing label") end
  do
    do goto b end
    n = n + 100
    ::b::
  end
end
print(n)

-- Without a label of its own block, a goto still jumps back to the enclosing block's.
local i = 0
::c::
i = i + 1
do
  if i < 3 then goto c end
end
print(i)

-- A label in a block that has ended is out of sight.
print(loadstring("do ::d:: end goto d"))
