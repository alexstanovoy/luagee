-- require looks in package.preload first, calling the loader with the module's name, as Lua 5.1 does; it reads
-- package.preload from the package table it was made with, whatever the global `package` becomes.
package.preload.greeting = function(...)
  print("loading", ...)
  return {hello = "world"}
end
local g = require "greeting"
print(g.hello, require "greeting" == g, package.loaded.greeting == g)
package.preload.nothing = function() end
print(require "nothing", package.loaded.nothing)
package.preload.sets = function(name) package.loaded[name] = "set by the loader" end
print(require "sets")
package.preload.failing = function() error("broken loader") end
print(pcall(require, "failing"))
local p = package
package = {}
p.preload.later = function() return 42 end
print(require "later")
package = p
package.preload = "not a table"
print(pcall(require, "another"))
package = p
package.preload = {}
-- A module declared by module(...) while require loads it becomes the loaded value.
package.preload.declared = function(...)
  module(..., package.seeall)
  function twice(x) return 2 * x end
end
local d = require "declared"
print(type(d), d == declared, d._NAME, d.twice(21), package.loaded.declared == d)
