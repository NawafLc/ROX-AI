--!strict
-- Runs only on Roblox's servers: checks the built place really starts up.
return {
	remoteOnly = true,
	["server script is in place"] = function(t)
		local sss = game:GetService("ServerScriptService")
		t.ok(sss:FindFirstChild("Server") ~= nil, "ServerScriptService.Server exists")
	end,
	-- Rojo ignores a "Position" property: place parts with "CFrame" or they all land at (0, 0, 0)
	["nothing covers the spawn (seen from above)"] = function(t)
		local spawn = workspace:FindFirstChildWhichIsA("SpawnLocation", true)
		t.ok(spawn ~= nil, "there is a SpawnLocation")
		if spawn then
			local hit = workspace:Raycast(spawn.Position + Vector3.new(0, 60, 0), Vector3.new(0, -120, 0))
			t.ok(hit ~= nil and hit.Instance == spawn, "the spawn is covered by " .. (if hit then hit.Instance:GetFullName() else "nothing"))
		end
	end,
}
