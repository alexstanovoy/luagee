-- LuaJIT's equal-bound and NaN-bound results are independent of the NaN step sign.
for _,step in ipairs({tonumber('nan'),0/0,-(0/0),math.huge-math.huge}) do
 for _,range in ipairs({{1,1},{-1,-1},{0/0,1},{1,0/0}}) do
  local n=0; for i=range[1],range[2],step do n=n+1; if n==2 then break end end
  print(n)
 end
end
