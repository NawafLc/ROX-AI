--!strict
-- Pure game logic lives in ReplicatedStorage.Shared so it can be unit tested without a server.
local Currency = {}

export type Wallet = { coins: number }

function Currency.new(start: number?): Wallet
	return { coins = math.max(0, math.floor(start or 0)) }
end

function Currency.add(wallet: Wallet, amount: number): number
	assert(amount >= 0, "amount must not be negative")
	wallet.coins += math.floor(amount)
	return wallet.coins
end

function Currency.spend(wallet: Wallet, amount: number): boolean
	if amount < 0 or wallet.coins < amount then
		return false
	end
	wallet.coins -= amount
	return true
end

-- 1234 -> "1.2K", 2500000 -> "2.5M"
function Currency.format(n: number): string
	if n >= 1e9 then
		return string.format("%.1fB", n / 1e9)
	elseif n >= 1e6 then
		return string.format("%.1fM", n / 1e6)
	elseif n >= 1e3 then
		return string.format("%.1fK", n / 1e3)
	end
	return tostring(math.floor(n))
end

return Currency
