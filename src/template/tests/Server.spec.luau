--!strict
-- Runs only on Roblox's servers: checks the built place really starts up.
return {
	remoteOnly = true,
	["server script is in place"] = function(t)
		local sss = game:GetService("ServerScriptService")
		t.ok(sss:FindFirstChild("Server") ~= nil, "ServerScriptService.Server exists")
	end,
	["baseplate exists"] = function(t)
		t.ok(workspace:FindFirstChild("Baseplate") ~= nil)
	end,
}
