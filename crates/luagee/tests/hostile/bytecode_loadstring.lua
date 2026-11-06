-- expect: error nil value
return loadstring(string.dump(function() return "bytecode ran" end))()
