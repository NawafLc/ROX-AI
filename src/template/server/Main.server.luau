--!strict
local Players = game:GetService("Players")
local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Currency = require(ReplicatedStorage.Shared.Currency)

local wallets: { [Player]: Currency.Wallet } = {}

Players.PlayerAdded:Connect(function(player)
	local wallet = Currency.new(0)
	wallets[player] = wallet

	local stats = Instance.new("Folder")
	stats.Name = "leaderstats"
	stats.Parent = player

	local coins = Instance.new("IntValue")
	coins.Name = "Coins"
	coins.Value = wallet.coins
	coins.Parent = stats
end)

Players.PlayerRemoving:Connect(function(player)
	wallets[player] = nil
end)

print("[game] server started")
