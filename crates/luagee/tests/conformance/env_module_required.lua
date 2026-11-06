-- A file module that declares itself with module(...) is what require returns.
local m = require "modules.env_module_required"
print(type(m), m == modules.env_module_required, m._NAME, m.value, package.loaded["modules.env_module_required"] == m)
print(package.loaded._NAME)
