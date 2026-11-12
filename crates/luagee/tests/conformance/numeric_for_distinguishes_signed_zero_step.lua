-- LuaJIT chooses direction by the step sign bit, including signed zero, unlike PUC Lua 5.1.
for _, step in ipairs({0.0,-0.0,'0','-0'}) do
 for _, range in ipairs({{1,2},{2,1},{1,1},{0,0},{-0.0,-0.0},{-1,1},{1,-1}}) do
  local n=0
  for i=range[1],range[2],step do
   n=n+1
   print('entered',i,1/i)
   if n==2 then break end
  end
  print('iterations',n)
 end
end
for _, range in ipairs({{0/0,1},{1,0/0},{math.huge,math.huge},{-math.huge,-math.huge}}) do
 for _, step in ipairs({0.0,-0.0}) do
  local n=0; for i=range[1],range[2],step do n=n+1; break end
  print('nonfinite',n)
 end
end
