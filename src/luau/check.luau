-- Local checks with Lune: every script compiles, then the unit tests run. Prints one ROXAI_RESULT line.
local fs = require("@lune/fs")
local luau = require("@lune/luau")
local process = require("@lune/process")
local roblox = require("@lune/roblox")
local serde = require("@lune/serde")

local game = roblox.deserializePlace(fs.readFile(process.args[1]))

local scripts, syntax = 0, {}
eachInstance(game, function(d)
	if d.ClassName == "Script" or d.ClassName == "LocalScript" or d.ClassName == "ModuleScript" then
		scripts += 1
		local ok, err = pcall(luau.compile, d.Source)
		if not ok then
			local first = string.match(tostring(err), "^[^\n]*") or tostring(err)
			table.insert(syntax, { script = d:GetFullName(), error = first })
		end
	end
end)

-- Roblox data types (Vector3, Color3, CFrame, UDim2, Enum, Instance, ...) like on a real server
local function withRobloxTypes(env: { [string]: any }): { [string]: any }
	for k, v in roblox :: any do
		if type(k) == "string" and string.match(k, "^%u") then
			env[k] = v
		end
	end
	return env
end

-- Roblox-style require(ModuleScript) on top of the place file
local cache = {}
local function load(mod: Instance): any
	local key = mod:GetFullName()
	if cache[key] ~= nil then
		return cache[key]
	end
	local env = withRobloxTypes({ script = mod, game = game, workspace = game:FindFirstChild("Workspace") })
	env.require = function(target)
		if type(target) == "userdata" and target.ClassName == "ModuleScript" then
			return load(target)
		end
		error("local tests can only require ModuleScripts (use remoteOnly for anything else)", 2)
	end
	local fn = luau.load(mod.Source, { debugName = key, environment = env })
	local value = fn()
	cache[key] = value
	return value
end

local tests = if #syntax == 0 then runSpecs(findSpecs(game), load, false) else {}
local failed = 0
for _, r in tests do
	if not r.ok then
		failed += 1
	end
end
print("ROXAI_RESULT " .. serde.encode("json", { scripts = scripts, syntax = syntax, tests = tests }))
process.exit(if #syntax > 0 or failed > 0 then 1 else 0)
