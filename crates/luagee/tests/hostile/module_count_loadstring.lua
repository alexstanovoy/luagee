-- expect: limit module_count
for i = 1, 5000 do loadstring("return " .. i) end
