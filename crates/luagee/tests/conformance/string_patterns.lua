-- Lua patterns: classes, sets, quantifiers, anchors, captures, position captures, %b, %f, backreferences.
local function m(s, p, init) return s:match(p, init) end
print(m("hello 123", "%a+"), m("hello 123", "%d+"), m("hello 123", "%s"), m("Hello", "%u"), m("Hello", "%l+"), m("a_b", "%w+"), m("0xFF", "%x+"), m("a,b", "%p"))
print(m("a\tb", "%c") == "\t", m("abc", "%A") == nil, m("abc1", "%D+"), m("ab cd", "%S+"), m("AbC", "%U+"), m("AbC", "%L+"), m("a_b-c", "%W+"), m("ab12", "%X+"))
print(m("abc", "."), m("abc", ".-"), m("abc", ".*"), m("abc", ".+"), m("abc", "a?"), m("abc", "x?"), m("abc", "x*"), m("abc", "b*"), m("abc", "^b"), m("abc", "^a"))
print(m("abc", "c$"), m("abc", "b$"), m("abc", "^abc$"), m("abc", "^$"), m("", "^$"), m("a$b", "a$b"), m("a^b", "a^b"), m("a$", "a%$"), m("^a", "%^a"))
print(m("aaa", "a-"), m("aaa", "a-$"), m("aaab", "a-b"), m("xaaay", "xa*y"), m("xy", "xa*y"), m("xaay", "xa?y"), m("xay", "xa?y"))
print(m("abc123def", "%d+"), m("abc123def", "%a+", 4), m("abc123def", "(%a+)(%d+)(%a+)"))
print(m("key=value", "(%w+)=(%w+)"), m("key = value", "(%w+)%s*=%s*(%w+)"), m("  trim  ", "^%s*(.-)%s*$") .. "|")
print(m("abc", "[a-b]+"), m("abc", "[^a-b]+"), m("a-c", "[a%-c]+"), m("a]c", "[]a]+"), m("a^c", "[%^a]+"), m("abc", "[%a]+"), m("123", "[%a%d]+"), m("-", "[a-]"), m("-", "[-a]"))
print(m("x", "[]]") == nil, m("]", "[]]"), m("a", "[^]]"), m("^", "[%^]"), m("a-z", "[a-z]+"), m("A-Z", "[a-zA-Z]+"), m("é", "[\128-\255]+") == "é")
print(m("f(a(b)c)d", "%b()"), m("[[x]]", "%b[]"), m("(", "%b()"), m("a(b", "%b()"), m("((x))", "%b()"), m("{a{b}}", "%b{}"), m("xx", "%bxx"))
print(m("THE (quick) fox", "%f[%a]%a+"), m("THE (quick) fox", "%f[%l]%l+"), m("hello world", "%f[%w]%w+", 2), m("a", "%f[%z]") , m("ab", "%f[%z]"))
print(("THE (quick) fox"):find("%f[%a]%a+", 5), ("hello"):find("%f[%a]"), ("hello"):find("%f[^%a]"), ("a b"):find("%f[%s]"))
print(m("abab", "(ab)%1"), m("abcabc", "(abc)%1"), m("abcabd", "(abc)%1"), m("xyzxyz", "(x)(y)(z)%1%2%3"), m("aa", "(a)%1"), m("ab", "(a)%1"))
print(m("hello", "()"), m("hello", "()ll()"), m("hello", "h()"), m("hello", "()", 3), m("hello", "(h)(e)()"))
print(m("hello", "((h)(e))"), m("abc", "(a)(b)(c)"), m("abc", "(((a)))"))
print(m("abc", "(a)(x)?") , m("abc", "a(.)"), m("abc", "(.)(.)(.)"), m("", "(.*)") == "")
print(select("#", m("abc", "(a)(b)")), select("#", m("abc", "abc")), select("#", m("abc", "x")), select("#", m("abc", "()")))
print(("a1b2c3"):gsub("%d", "#"), ("a1b2c3"):gsub("%a", "%0%0"), ("abc"):gsub("", "-"), ("abc"):gsub(".", "%%"))
for word in ("one two  three"):gmatch("%a+") do print(word) end
for k, v in ("a=1, b=2, c=3"):gmatch("(%w+)=(%w+)") do print(k, v) end
for pos in ("abc"):gmatch("()") do print(pos) end
for a in ("abc"):gmatch("") do print("[" .. a .. "]") end
for a, b in ("abc"):gmatch("(.)(.)") do print(a, b) end
local words = {}
for w in ("The quick brown fox"):gmatch("%S+") do words[#words + 1] = w:lower() end
print(table.concat(words, ","))
local cnt = 0
for _ in ("aaa"):gmatch("a*") do cnt = cnt + 1 end
print(cnt)
cnt = 0
for _ in ("abc"):gmatch(".-") do cnt = cnt + 1 end
print(cnt)
cnt = 0
for _ in ("a,b,,c"):gmatch("([^,]*)") do cnt = cnt + 1 end
print(cnt)
print(("x"):match("(x)(y?)"), ("x"):match("^(x)$"), ("  x"):match("^%s*(%S)"))
print(("2024-01-15"):match("(%d+)-(%d+)-(%d+)"))
print(("path/to/file.txt"):match("([^/]+)$"), ("path/to/file.txt"):match("(.+)/"), ("file.txt"):match("%.(%w+)$"), ("noext"):match("%.(%w+)$"))
print(("CamelCaseString"):gsub("(%u)", " %1"))
print(("  a b  "):gsub("^%s+", ""), ("  a b  "):gsub("%s+$", ""), (("  a b  "):gsub("^%s*(.-)%s*$", "%1")))
print(("abc"):find("b", 1, true), ("a.c"):find(".", 1, true), ("abc"):find("(b)(c)"), ("abc"):find("x"), ("abc"):find("c", -1))
print(("aXb"):find("%u"), ("aXb"):find("[%u]"), ("a b"):find("%s"), ("a\nb"):find("\n"), ("a\nb"):find("%c"))
print(pcall(string.find, "a", "[") == false, pcall(string.find, "a", "(") == false, pcall(string.find, "a", ")") == false, pcall(string.find, "a", "%") == false)
print(("a.b.c"):gsub("%.", "/"), ("a b c"):gsub(" ", "_", 1), ("abc"):gsub("b", "%%1"), ("hello world"):gsub("o", "0", 0))
print(("x"):rep(5):gsub("x", "y", 3))
print(("%d"):gsub("%%d", "N"), ("a%b"):gsub("%%", "pct"), ("50%"):gsub("%%$", " percent"))
print(("abc"):match(".-(b)"), ("abc"):match("a(.-)c"), ("abcabc"):match("a(.-)c"), ("abcabc"):match("a(.*)c"), ("<a><b>"):match("<(.-)>"), ("<a><b>"):match("<(.*)>"))
print(("one;two;;four"):find(";", 1, true), ("a,b"):find(",", 1, true))
print(string.find("abc", "b", nil, true), string.find("abc", "b", nil), string.match("abc", "b", nil), string.match("abc", "()", -1))
print(("[tag]"):match("%[(.-)%]"), ("a.b"):match("^(.-)%.(.*)$"), ("k: v"):match("^(%w+):%s*(.*)$"))
print(("x1y22z333"):gsub("%d+", function(d) return "<" .. #d .. ">" end))
