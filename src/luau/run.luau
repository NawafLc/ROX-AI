-- Run one Luau snippet against the built place (Lune, this PC) and return what it prints.
-- Agents use it for quick checks and economy simulations instead of writing throwaway failing tests.
local fs = require("@lune/fs")
local luau = require("@lune/luau")
local process = require("@lune/process")
local roblox = require("@lune/roblox")
local serde = require("@lune/serde")

local game = roblox.deserializePlace(fs.readFile(process.args[1]))
local code = fs.readFile(process.args[2])

local out = {}
local size = 0
local function emit(...)
	local parts = table.pack(...)
	for i = 1, parts.n do
		parts[i] = tostring(parts[i])
	end
	local line = table.concat(parts, "\t", 1, parts.n)
	size += #line
	if size <= 20000 then
		table.insert(out, line)
	end
end

-- Roblox data types (Vector3, Color3, CFrame, UDim2, Enum, Instance, ...) like on a real server
local function withRobloxTypes(env: { [string]: any }): { [string]: any }
	for k, v in roblox :: any do
		if type(k) == "string" and string.match(k, "^%u") then
			env[k] = v
		end
	end
	return env
end

-- Roblox-style require(ModuleScript) on top of the place file (same rules as the unit tests)
local cache = {}
local function load(mod: Instance): any
	local key = mod:GetFullName()
	if cache[key] ~= nil then
		return cache[key]
	end
	local env = withRobloxTypes({ script = mod, game = game, workspace = game:FindFirstChild("Workspace"), print = emit, warn = emit })
	env.require = function(target)
		if type(target) == "userdata" and target.ClassName == "ModuleScript" then
			return load(target)
		end
		error("local runs can only require ModuleScripts", 2)
	end
	local fn = luau.load(mod.Source, { debugName = key, environment = env })
	local value = fn()
	cache[key] = value
	return value
end

local env = withRobloxTypes({
	game = game,
	workspace = game:FindFirstChild("Workspace"),
	print = emit,
	warn = emit,
	require = function(target)
		if type(target) == "userdata" and target.ClassName == "ModuleScript" then
			return load(target)
		end
		error("require a ModuleScript, e.g. require(game.ReplicatedStorage.Shared.Config)", 2)
	end,
})
local ok, err = pcall(function()
	local fn = luau.load(code, { debugName = "snippet", environment = env })
	local r = table.pack(fn())
	if r.n > 0 then
		emit("=>", table.unpack(r, 1, r.n))
	end
end)
print("ROXAI_RUN " .. serde.encode("json", { ok = ok, error = if ok then nil else tostring(err), output = table.concat(out, "\n"), cut = size > 20000 }))
