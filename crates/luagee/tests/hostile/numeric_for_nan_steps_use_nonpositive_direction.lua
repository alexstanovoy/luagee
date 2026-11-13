-- expect: ok nan step directions stable
-- Sandbox determinism rule: NaN always selects limit <= index, regardless of sign.
-- Unlike unequal bounds in LuaJIT, these expectations are defined by this documented rule.
for _,step in ipairs({tonumber('nan'),0/0,-(0/0),math.huge-math.huge}) do
 for _,range in ipairs({{1,2,0},{2,1,1},{1,1,1}}) do
  local n=0; for i=range[1],range[2],step do n=n+1; assert(n<=1) end
  assert(n==range[3])
 end
end
print('nan step directions stable')
