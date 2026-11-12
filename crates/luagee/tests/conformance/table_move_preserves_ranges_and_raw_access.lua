-- table.move is a LuaJIT extension to Lua 5.1. It uses raw signed-32-bit keys.
for _, range in ipairs({{1,3,2}, {2,4,1}, {1,4,1}, {3,1,8}, {-2,1,-1}}) do
 local a={10,20,30,40,[-2]=7,[-1]=8,[0]=9}
 local result=table.move(a,unpack(range))
 print(result==a,a[-2],a[-1],a[0],a[1],a[2],a[3],a[4],a[5])
end
local meta={__index=function() error('unexpected read') end, __newindex=function() error('unexpected write') end}
local source=setmetatable({[1]='x',[3]='z'},meta)
local dest=setmetatable({[0]='old',[1]='old'},meta)
print(table.move(source,1,3,-1,dest)==dest,rawget(dest,-1),rawget(dest,0),rawget(dest,1))
for _, n in ipairs({1.9, -1.9, -0.0, 2^31, 2^32, 0/0}) do
 local a={[1]=11,[-1]=22,[0]=33,[-2147483648]=44}; local d={}
 table.move(a,n,n,0,d)
 print(n,d[0])
end
local d={}; table.move({9},1,1,0/0,d); print(d[-2147483648])
print(table.move({7,8},'1','2','3')[3])
for _, args in ipairs({{}, {false}, {{},false}, {{},1,false}, {{},1,1,false}, {{},1,0,1,false}}) do
 local ok,err=pcall(table.move,unpack(args)); print(ok,err:match('bad argument.*'))
end
for _, range in ipairs({{.1,2.1,0},{-.1,1.9,0},{.9999999999999999,3,0},{1,2.9,1.1},{-2^53,-2^53,0}}) do
 local a={[-1]=9,[0]=10,[1]=11,[2]=12,[3]=13,[-2147483648]=14}; local b={}
 table.move(a,range[1],range[2],range[3],b)
 print(b[-2147483648],b[-1],b[0],b[1],b[2],b[3])
end
local meta={__eq=function() print('equality called'); return true end}
local a,b=setmetatable({11,22,33},meta),setmetatable({},meta)
table.move(a,1,3,2,b); print(b[2],b[3],b[4])
