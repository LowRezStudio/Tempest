using System.Text;

namespace Tempest.CLI.Server;

// ----------------------------------------------------------------------
// Command handler — parses and dispatches interactive server commands
// ----------------------------------------------------------------------

/// <summary>
/// Parses and executes commands typed into the interactive server console.
/// </summary>
internal class CommandHandler
{
	private readonly LobbyState _state;
	private readonly EmbeddedServer _server;
	private readonly LobbyServiceImpl _lobbyService;
	private readonly IPBanStore _banStore;
	private readonly CancellationTokenSource _cts;

	public CommandHandler(LobbyState state, EmbeddedServer server, LobbyServiceImpl lobbyService, IPBanStore banStore, CancellationTokenSource cts)
	{
		_state = state;
		_server = server;
		_lobbyService = lobbyService;
		_banStore = banStore;
		_cts = cts;
	}

	/// <summary>
	/// Parse and execute a single command line.
	/// </summary>
	public void Execute(string input)
	{
		var parts = ParseCommand(input);
		if (parts.Length == 0) return;

		var cmd = parts[0].ToLowerInvariant();
		var args = parts.AsSpan(1);

		switch (cmd)
		{
			case "help":
			case "?":
				ShowHelp();
				break;

			case "status":
				ShowStatus();
				break;

			case "players":
			case "list":
				ListPlayers();
				break;

			case "kick":
				if (args.Length > 0)
				{
					var reason = args.Length > 1 ? string.Join(" ", args.Slice(1).ToArray()) : "Kicked by server";
					KickPlayer(args[0], reason);
				}
				else
					Console.WriteLine("Usage: kick <playerId> [reason]");
				break;

			case "ban":
				if (args.Length > 0)
					BanPlayer(args);
				else
					Console.WriteLine("Usage: ban <playerId|ip> [reason]");
				break;

			case "unban":
				if (args.Length > 0)
					UnbanIP(args[0]);
				else
					Console.WriteLine("Usage: unban <ip>");
				break;

			case "banlist":
			case "bans":
				ListBans();
				break;

			case "say":
			case "chat":
				if (args.Length > 0)
					SendChat(string.Join(" ", args.ToArray()));
				else
					Console.WriteLine("Usage: say <message>");
				break;

			case "stop":
			case "exit":
			case "quit":
			case "kill":
				Console.WriteLine("Shutting down server...");
				_cts.Cancel();
				break;

			case "clear":
			case "cls":
				Console.Clear();
				break;

			default:
				Console.WriteLine($"Unknown command: '{cmd}'. Type 'help' for available commands.");
				break;
		}
	}

	// ------------------------------------------------------------------
	// Command implementations
	// ------------------------------------------------------------------

	private static void ShowHelp()
	{
		Console.WriteLine("Available commands:");
		Console.WriteLine("  help, ?                 Show this help");
		Console.WriteLine("  status                  Show server status");
		Console.WriteLine("  players, list           List connected players");
		Console.WriteLine("  kick <playerId> [reason] Kick a player by ID");
		Console.WriteLine("  ban <playerId|ip> [reason] Ban a player by ID or IP");
		Console.WriteLine("  unban <ip>              Unban an IP address");
		Console.WriteLine("  banlist, bans           List all IP bans");
		Console.WriteLine("  say <message>           Broadcast a chat message to all players");
		Console.WriteLine("  clear, cls              Clear the console");
		Console.WriteLine("  stop, exit, quit        Stop the server");
	}

	private void ShowStatus()
	{
		var info = _state.GetInfoEvent().Info;
		Console.WriteLine($"Server:   {info.Name}");
		Console.WriteLine($"Game:     {info.Game} v{info.Version}");
		Console.WriteLine($"Gamemode: {info.Gamemode}");
		Console.WriteLine($"Players:  {info.Players.Count}/{info.MaxPlayers}");
		Console.WriteLine($"Password: {(info.PasswordRequired ? "Yes" : "No")}");

		var banCount = _banStore.GetAllBans().Count;
		if (banCount > 0)
			Console.WriteLine($"IP bans:  {banCount}");
	}

	private void ListPlayers()
	{
		var info = _state.GetInfoEvent().Info;
		if (info.Players.Count == 0)
		{
			Console.WriteLine("No players connected.");
			return;
		}

		Console.WriteLine($"Players ({info.Players.Count}/{info.MaxPlayers}):");
		foreach (var player in info.Players)
		{
			var champ = !string.IsNullOrEmpty(player.Champion) ? $" [{player.Champion}]" : "";
			var ip = _lobbyService.GetPlayerIP(player.Id) ?? "?";
			Console.WriteLine($"  {player.DisplayName}  (ID: {player.Id})  IP: {ip}  Team: {player.TaskForce}{champ}");
		}
	}

	private void KickPlayer(string playerId, string reason)
	{
		if (_state.KickPlayer(playerId, reason))
			Console.WriteLine($"Kicked player {playerId}.");
		else
			Console.WriteLine($"Player '{playerId}' not found.");
	}

	private void BanPlayer(Span<string> args)
	{
		var target = args[0];
		var reason = args.Length > 1 ? string.Join(" ", args.Slice(1).ToArray()) : "Banned by server";

		// Try to resolve as player ID first, then as raw IP
		string? ip = null;
		string? playerName = null;

		var info = _state.GetInfoEvent().Info;
		foreach (var player in info.Players)
		{
			if (player.Id == target || player.DisplayName.Equals(target, StringComparison.OrdinalIgnoreCase))
			{
				playerName = player.DisplayName;
				ip = _lobbyService.GetPlayerIP(player.Id);
				break;
			}
		}

		// If not found as player, treat target as raw IP
		if (ip == null)
		{
			ip = target;
		}

		if (_banStore.Ban(ip, reason, "Console"))
		{
			Console.WriteLine($"Banned IP {ip}{(playerName != null ? $" (player {playerName})" : "")}: {reason}");

			// Also kick the player if they're connected
			if (playerName != null)
			{
				foreach (var player in info.Players)
				{
					if (player.Id == target || player.DisplayName.Equals(target, StringComparison.OrdinalIgnoreCase))
					{
						_state.KickPlayer(player.Id, $"Banned: {reason}");
						break;
					}
				}
			}
		}
		else
		{
			Console.WriteLine($"IP {ip} is already banned.");
		}
	}

	private void UnbanIP(string ip)
	{
		if (_banStore.Unban(ip))
			Console.WriteLine($"Unbanned IP {ip}.");
		else
			Console.WriteLine($"IP {ip} is not banned.");
	}

	private void ListBans()
	{
		var bans = _banStore.GetAllBans();
		if (bans.Count == 0)
		{
			Console.WriteLine("No IP bans.");
			return;
		}

		Console.WriteLine($"IP bans ({bans.Count}):");
		foreach (var ban in bans)
		{
			Console.WriteLine($"  {ban.IP} — {ban.Reason} (by {ban.BannedBy}, {ban.BannedAt:yyyy-MM-dd HH:mm:ss} UTC)");
		}
	}

	private void SendChat(string message)
	{
		_state.SendChat("[SERVER]", message);
		Console.WriteLine($"[Server] {message}");
	}

	// ------------------------------------------------------------------
	// Simple command-line parser (handles quoted strings)
	// ------------------------------------------------------------------

	private static string[] ParseCommand(string input)
	{
		var parts = new List<string>();
		var current = new StringBuilder();
		var inQuotes = false;

		for (var i = 0; i < input.Length; i++)
		{
			var c = input[i];
			if (c == '"')
			{
				inQuotes = !inQuotes;
			}
			else if (c == ' ' && !inQuotes)
			{
				if (current.Length > 0)
				{
					parts.Add(current.ToString());
					current.Clear();
				}
			}
			else
			{
				current.Append(c);
			}
		}

		if (current.Length > 0)
			parts.Add(current.ToString());

		return parts.ToArray();
	}
}
