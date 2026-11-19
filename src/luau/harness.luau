-- ROX-AI test harness. The same code runs on this PC (Lune) and on Roblox's servers (Luau Execution).
-- A spec is a ModuleScript named "*.spec" that returns { ["test name"] = function(t) ... end, remoteOnly = bool? }.
local ROOTS = { "ReplicatedStorage", "ServerScriptService", "ServerStorage", "StarterPlayer", "ReplicatedFirst", "StarterGui", "Workspace" }

local function show(v: any): string
	if type(v) == "string" then
		return string.format("%q", v)
	end
	return tostring(v)
end

local function makeT()
	local t = {}
	function t.eq(actual: any, expected: any, msg: string?)
		if actual ~= expected then
			error((if msg then msg .. ": " else "") .. "expected " .. show(expected) .. ", got " .. show(actual), 2)
		end
	end
	function t.ok(value: any, msg: string?)
		if not value then
			error(msg or "expected a true value", 2)
		end
	end
	function t.fails(fn: () -> (), msg: string?)
		if pcall(fn) then
			error(msg or "expected an error", 2)
		end
	end
	return t
end

local function eachInstance(root: Instance, fn: (Instance) -> ())
	for _, name in ROOTS do
		local svc = root:FindFirstChild(name)
		if svc then
			for _, d in svc:GetDescendants() do
				fn(d)
			end
		end
	end
end

local function findSpecs(root: Instance)
	local specs = {}
	eachInstance(root, function(d)
		if d.ClassName == "ModuleScript" and string.sub(d.Name, -5) == ".spec" then
			table.insert(specs, d)
		end
	end)
	table.sort(specs, function(a, b)
		return a:GetFullName() < b:GetFullName()
	end)
	return specs
end

local function runSpecs(specs: { Instance }, load: (Instance) -> any, remote: boolean)
	local results = {}
	for _, mod in specs do
		local name = mod:GetFullName()
		local ok, suite = pcall(load, mod)
		if not ok then
			table.insert(results, { spec = name, test = "(loading the spec)", ok = false, error = tostring(suite) })
			continue
		end
		if type(suite) ~= "table" then
			table.insert(results, { spec = name, test = "(loading the spec)", ok = false, error = "a spec must return a table of tests" })
			continue
		end
		if suite.remoteOnly and not remote then
			table.insert(results, { spec = name, test = "(runs on Roblox's servers only)", ok = true, skipped = true })
			continue
		end
		local names = {}
		for k, v in suite do
			if type(v) == "function" then
				table.insert(names, k)
			end
		end
		table.sort(names)
		for _, n in names do
			local passed, err = pcall(suite[n], makeT())
			table.insert(results, { spec = name, test = n, ok = passed, error = if passed then nil else tostring(err) })
		end
	end
	return results
end
