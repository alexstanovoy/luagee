-- Program: classic algorithms (sieve, gcd, sorting, search, matrices, permutations, memoization, big sums).
local function sieve(n)
  local is = {}
  for i = 2, n do is[i] = true end
  for i = 2, math.floor(math.sqrt(n)) do if is[i] then for j = i * i, n, i do is[j] = false end end end
  local primes = {}
  for i = 2, n do if is[i] then primes[#primes + 1] = i end end
  return primes
end
local primes = sieve(100)
print(#primes, primes[1], primes[#primes], table.concat(primes, ",", 1, 10))
local function gcd(a, b) while b ~= 0 do a, b = b, a % b end return a end
print(gcd(48, 18), gcd(17, 5), gcd(0, 5), gcd(100, 100), gcd(2^40, 2^20))
local function bsearch(t, x) local lo, hi = 1, #t while lo <= hi do local mid = math.floor((lo + hi) / 2) if t[mid] == x then return mid elseif t[mid] < x then lo = mid + 1 else hi = mid - 1 end end return nil end
print(bsearch(primes, 53), bsearch(primes, 54), bsearch(primes, 2), bsearch(primes, 97), bsearch({}, 1))
local function quicksort(t)
  if #t <= 1 then return t end
  local pivot, less, more = t[1], {}, {}
  for i = 2, #t do if t[i] < pivot then less[#less + 1] = t[i] else more[#more + 1] = t[i] end end
  local r = quicksort(less)
  r[#r + 1] = pivot
  for _, v in ipairs(quicksort(more)) do r[#r + 1] = v end
  return r
end
print(table.concat(quicksort({5, 3, 8, 1, 9, 2, 7, 3, 5}), " "))
local function matmul(a, b)
  local n, m, p = #a, #b, #b[1]
  local c = {}
  for i = 1, n do c[i] = {} for j = 1, p do local s = 0 for k = 1, m do s = s + a[i][k] * b[k][j] end c[i][j] = s end end
  return c
end
local m = matmul({{1, 2}, {3, 4}}, {{5, 6}, {7, 8}})
print(m[1][1], m[1][2], m[2][1], m[2][2])
local function permutations(t)
  return coroutine.wrap(function()
    local function permute(n)
      if n == 0 then coroutine.yield(table.concat(t, "")) return end
      for i = 1, n do t[i], t[n] = t[n], t[i] permute(n - 1) t[i], t[n] = t[n], t[i] end
    end
    permute(#t)
  end)
end
local perms = {}
for p in permutations({"a", "b", "c"}) do perms[#perms + 1] = p end
table.sort(perms)
print(table.concat(perms, " "))
local memo = {}
local function fib(n) if n < 3 then return 1 end if memo[n] then return memo[n] end local v = fib(n - 1) + fib(n - 2) memo[n] = v return v end
print(fib(10), fib(50), fib(80), fib(90))
local function collatz(n) local steps = 0 while n ~= 1 do if n % 2 == 0 then n = n / 2 else n = 3 * n + 1 end steps = steps + 1 end return steps end
print(collatz(27), collatz(97), collatz(871))
local function to_base(n, b) local digits = "0123456789abcdefghijklmnopqrstuvwxyz" local s = "" repeat local d = n % b s = digits:sub(d + 1, d + 1) .. s n = (n - d) / b until n == 0 return s end
print(to_base(255, 2), to_base(255, 16), to_base(0, 2), to_base(123456789, 36), tonumber(to_base(123456789, 36), 36))
local sum, sumsq = 0, 0
for i = 1, 100000 do sum = sum + i sumsq = sumsq + i * i end
print(sum, sumsq, sum / 100000)
local function hanoi(n, a, b, c, moves) if n == 0 then return moves end hanoi(n - 1, a, c, b, moves) moves[#moves + 1] = a .. c return hanoi(n - 1, b, a, c, moves) end
local mv = hanoi(4, "A", "B", "C", {})
print(#mv, mv[1], mv[8], mv[15])
local function lcs(a, b)
  local dp = {}
  for i = 0, #a do dp[i] = {} for j = 0, #b do dp[i][j] = 0 end end
  for i = 1, #a do for j = 1, #b do
    if a:sub(i, i) == b:sub(j, j) then dp[i][j] = dp[i - 1][j - 1] + 1 else dp[i][j] = math.max(dp[i - 1][j], dp[i][j - 1]) end
  end end
  return dp[#a][#b]
end
print(lcs("AGGTAB", "GXTXAYB"), lcs("", "abc"), lcs("abc", "abc"))
local graph = {a = {"b", "c"}, b = {"d"}, c = {"d"}, d = {}}
local visited, order = {}, {}
local function dfs(n) if visited[n] then return end visited[n] = true for _, m2 in ipairs(graph[n]) do dfs(m2) end order[#order + 1] = n end
dfs("a")
print(table.concat(order, ""))
local d = 0
for i = 1, 20 do d = d + math.sin(i) * math.cos(i) end
print(string.format("%.6f", d))
