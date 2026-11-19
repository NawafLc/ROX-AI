-- Runs inside the test place on Roblox's servers (Open Cloud Luau Execution).
task.wait(1) -- let the server scripts start
local specs = findSpecs(game)
local results = runSpecs(specs, require, true)
return { tests = results, placeVersion = game.PlaceVersion, placeId = game.PlaceId }
