-- LuaJIT suppresses NaN plus and negative signs; space, width and case still apply.
for _, conversion in ipairs({'f','F','e','E','g','G','a','A'}) do
 for _, flags in ipairs({'','+',' ','-','0','#','+010',' 010','-+10','#10.0','10.2'}) do
  local template='%'..flags..conversion
  for _, value in ipairs({0/0,-(0/0),math.huge,-math.huge,0,-0.0}) do
   print(template,'['..string.format(template,value)..']')
  end
 end
 local ok,err=pcall(string.format,'%'..conversion,false)
 print(ok,err:match('bad argument.*'))
end
