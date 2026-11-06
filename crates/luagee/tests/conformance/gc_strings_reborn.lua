-- Strings that became garbage are made again (and kept) while the collector may be about to free them.
local made = 0
for round = 1, 300 do
  local t = {}
  for i = 1, 40 do t[i] = "k" .. i .. "_" .. round % 5 end
  t = nil
  local keep = {}
  for i = 1, 40 do keep[i] = "k" .. i .. "_" .. round % 5 end
  local more = {}
  for i = 1, 40 do more[i] = {i} end
  for i = 1, 40 do
    assert(keep[i] == "k" .. i .. "_" .. round % 5)
    made = made + #keep[i]
  end
end
print(made)
