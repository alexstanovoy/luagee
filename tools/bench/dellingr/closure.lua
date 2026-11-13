-- closure workload. See PROVENANCE.md for origin and modifications.
-- Repeated kernels produce a checked checksum; the queue uses a shorter repetition count.

local function kernel()
    local total = 0
    for i = 1, 500 do
        local x = i
        local f = function(y)
            return x + y
        end
        total = total + f(1)
    end
    return total
end

local K = 1000

function _bench()
    local acc = 0
    for _ = 1, K do
        acc = acc + kernel()
    end
    return acc
end

for i = 1, 30 do _bench() end
print("bench/closure: " .. tostring(_bench()))
