-- expect: limit fuel_per_call
local function f() local ok = pcall(f) return ok end while true do f() end
