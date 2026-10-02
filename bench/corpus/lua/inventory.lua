-- Tables, metatables, closures, varargs and long strings.
local Inventory = {}
Inventory.__index = Inventory

local function clamp(value, low, high)
  if value < low then return low end
  if value > high then return high end
  return value
end

function Inventory.new(limit)
  local self = setmetatable({}, Inventory)
  self.items = {}
  self.limit = limit or tonumber(os.getenv("INVENTORY_LIMIT") or "100")
  return self
end

function Inventory:add(sku, qty)
  assert(type(sku) == "string" and #sku > 0, "sku required")
  qty = clamp(qty or 1, 1, self.limit)
  self.items[sku] = (self.items[sku] or 0) + qty
  return self
end

function Inventory:total()
  local sum = 0
  for _, qty in pairs(self.items) do
    sum = sum + qty
  end
  return sum
end

function Inventory:each(...)
  local filters = { ... }
  return coroutine.wrap(function()
    for sku, qty in pairs(self.items) do
      local keep = true
      for i = 1, #filters do
        keep = keep and filters[i](sku, qty)
      end
      if keep then coroutine.yield(sku, qty) end
    end
  end)
end

Inventory.__tostring = function(self)
  return string.format("Inventory(%d units)", self:total())
end

local report = [=[
Inventory report — "quoted"
  with [[nested]] brackets kept
]=]

local deep = { a = { b = { c = { d = { 1, 2, { 3 } } } } } }

local inv = Inventory.new(10):add("café", 2):add("thé")
for sku, qty in inv:each(function(_, q) return q > 1 end) do
  print(sku, qty)
end
print(tostring(inv), report, deep.a.b.c.d[3][1])

return Inventory
