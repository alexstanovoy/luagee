-- Duplicate flags are accepted by LuaJIT; they are not width digits.
for _, flag in ipairs({'0','-','+',' ','#'}) do
 for _, count in ipairs({1,10,1000}) do
  local repeated=string.rep(flag,count)
  print('['..flag..']',count,'['..string.format('%'..repeated..'8.3d',12)..']')
  print('['..flag..']',count,'['..string.format('%'..repeated..'d',1)..']')
 end
end
for _, template in ipairs({'%0000000000000000000000d','%----99s','%+++100d','%.000d','%++','%---z','%---%'}) do
 local ok,result=pcall(string.format,template,1)
 print(ok,'['..result..']')
end
