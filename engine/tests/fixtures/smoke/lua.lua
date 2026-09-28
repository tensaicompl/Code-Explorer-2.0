local Greeter = {}

function Greeter.greet(name)
  return "hi " .. name
end

local function make()
  return Greeter
end

return { make = make }
