local M = {}

function M.map(t, f)
  local out = {}
  for i, v in ipairs(t) do
    out[i] = f(v, i)
  end
  return out
end

function M.memoize(f)
  local cache = setmetatable({}, { __mode = "k" })
  return function(x)
    local v = cache[x]
    if v == nil then
      v = f(x)
      cache[x] = v
    end
    return v
  end
end

M.fib = M.memoize(function(n)
  if n < 2 then return n end
  return M.fib(n - 1) + M.fib(n - 2)
end)

goto done
print("skipped")
::done::

return M
