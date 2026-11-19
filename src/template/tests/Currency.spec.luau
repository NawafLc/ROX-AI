--!strict
-- Every ModuleScript named *.spec returns a table of tests. Each test gets `t` with
-- t.eq(actual, expected, msg?), t.ok(value, msg?) and t.fails(fn, msg?).
-- They run twice: on this PC (Lune, no server needed) and on Roblox's servers (Luau Execution).
-- Set `remoteOnly = true` for tests that need a real running server (Players, DataStores, physics…).
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local Currency = require(ReplicatedStorage.Shared.Currency)

return {
	["new wallet starts at the given amount"] = function(t)
		t.eq(Currency.new(5).coins, 5)
		t.eq(Currency.new().coins, 0)
		t.eq(Currency.new(-3).coins, 0, "never negative")
	end,
	["add and spend"] = function(t)
		local w = Currency.new(10)
		t.eq(Currency.add(w, 5), 15)
		t.ok(Currency.spend(w, 15))
		t.ok(not Currency.spend(w, 1), "can't spend more than you have")
		t.fails(function()
			Currency.add(w, -1)
		end)
	end,
	["format big numbers"] = function(t)
		t.eq(Currency.format(999), "999")
		t.eq(Currency.format(1234), "1.2K")
		t.eq(Currency.format(2500000), "2.5M")
	end,
}
