-- Export the game's 3D world (every visible part) as JSON so ROX-AI can draw pictures of it.
-- args: place file, setup snippet (Luau that builds the runtime world, may be empty), output JSON path
local fs = require("@lune/fs")
local luau = require("@lune/luau")
local process = require("@lune/process")
local roblox = require("@lune/roblox")
local serde = require("@lune/serde")

local game = roblox.deserializePlace(fs.readFile(process.args[1]))
local setup = fs.readFile(process.args[2])

local function withRobloxTypes(env: { [string]: any }): { [string]: any }
	for k, v in roblox :: any do
		if type(k) == "string" and string.match(k, "^%u") then
			env[k] = v
		end
	end
	return env
end

local logs = {}
local function emit(...)
	local parts = table.pack(...)
	for i = 1, parts.n do
		parts[i] = tostring(parts[i])
	end
	if #logs < 40 then
		table.insert(logs, table.concat(parts, "\t", 1, parts.n))
	end
end

local cache = {}
local function load(mod: any): any
	local key = mod:GetFullName()
	if cache[key] ~= nil then
		return cache[key]
	end
	local env = withRobloxTypes({ script = mod, game = game, workspace = game:FindFirstChild("Workspace"), print = emit, warn = emit })
	env.require = function(target)
		if type(target) == "userdata" and target.ClassName == "ModuleScript" then
			return load(target)
		end
		error("renders can only require ModuleScripts", 2)
	end
	local fn = luau.load(mod.Source, { debugName = key, environment = env })
	local value = fn()
	cache[key] = value
	return value
end

local setupError = nil
if string.match(setup, "%S") then
	local env = withRobloxTypes({
		game = game,
		workspace = game:FindFirstChild("Workspace"),
		print = emit,
		warn = emit,
		require = function(target)
			if type(target) == "userdata" and target.ClassName == "ModuleScript" then
				return load(target)
			end
			error("require a ModuleScript, e.g. require(game.ServerScriptService.Server.WorldBuilder)", 2)
		end,
	})
	local ok, err = pcall(function()
		luau.load(setup, { debugName = "setup", environment = env })()
	end)
	if not ok then
		setupError = tostring(err)
	end
end

local SHAPES = { Block = "block", Ball = "ball", Cylinder = "cylinder", Wedge = "wedge", CornerWedge = "cornerwedge" }
local parts = {}
local skipped = 0
local ws = game:FindFirstChild("Workspace")
for _, d in (if ws then ws:GetDescendants() else {}) :: { any } do
	if d:IsA("BasePart") and not d:IsA("Terrain") then
		local t = d.Transparency
		if t < 0.98 then
			local shape = "block"
			if d.ClassName == "WedgePart" then
				shape = "wedge"
			elseif d.ClassName == "CornerWedgePart" then
				shape = "cornerwedge"
			elseif d:IsA("Part") then
				shape = SHAPES[d.Shape.Name] or "block"
			end
			-- Lune keeps `part.Position = …` / `part.Orientation = …` as plain values without moving the part
			-- (on Roblox they move it), so apply them here like Roblox would
			local cframe = d.CFrame
			local okO, ori = pcall(function()
				return d.Orientation
			end)
			if okO and ori then
				cframe = roblox.CFrame.new(cframe.Position)
					* roblox.CFrame.fromOrientation(math.rad(ori.X), math.rad(ori.Y), math.rad(ori.Z))
			end
			local okP, pos = pcall(function()
				return d.Position
			end)
			if okP and pos and cframe.Position.Magnitude < 1e-6 then
				cframe = roblox.CFrame.new(pos) * (cframe - cframe.Position)
			end
			local c = table.pack(cframe:GetComponents())
			local cf = {}
			for i = 1, 12 do
				cf[i] = math.floor(c[i] * 1000 + 0.5) / 1000
			end
			local col = d.Color
			table.insert(parts, {
				n = d:GetFullName(),
				s = shape,
				cf = cf,
				size = { d.Size.X, d.Size.Y, d.Size.Z },
				c = { col.R, col.G, col.B },
				t = t,
				neon = d.Material.Name == "Neon",
				spawn = d:IsA("SpawnLocation"),
			})
			if #parts >= 20000 then
				break
			end
		else
			skipped += 1
		end
	end
end

fs.writeFile(process.args[3], serde.encode("json", { parts = parts, invisible = skipped, setupError = setupError, logs = logs }))
print("ROXAI_SCENE " .. #parts)
