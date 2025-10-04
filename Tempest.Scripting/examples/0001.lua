local PlayerController = game.Engine.PlayerController
local TgPlayerController = game.TgGame.TgPlayerController
local TgGame = game.TgGame.TgGame

TgGame.PostLogin:Connect(function(this, newPlayer)
    if (TgPlayerController:Is(newPlayer)) then
        print("newPlayer isn't TgPlayerController")

        return
    end

    newPlayer.PlayerName = "Kyiro"
end)