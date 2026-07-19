using System.Collections.Concurrent;

namespace Tempest.CLI.Server;

internal sealed class IPBanEntry
{
	public string IP { get; init; } = string.Empty;
	public string Reason { get; init; } = string.Empty;
	public string BannedBy { get; init; } = string.Empty;
	public DateTime BannedAt { get; init; } = DateTime.UtcNow;
}

internal sealed class IPBanStore
{
	private readonly ConcurrentDictionary<string, IPBanEntry> _bans = new();
	private readonly ILogger<IPBanStore> _logger;

	public IPBanStore(ILogger<IPBanStore> logger)
	{
		_logger = logger;
	}

	public bool IsBanned(string ip)
	{
		return _bans.ContainsKey(NormalizeIP(ip));
	}

	public IPBanEntry? GetBan(string ip)
	{
		_bans.TryGetValue(NormalizeIP(ip), out var entry);
		return entry;
	}

	public bool Ban(string ip, string reason, string bannedBy)
	{
		ip = NormalizeIP(ip);
		if (_bans.ContainsKey(ip))
			return false;

		var entry = new IPBanEntry
		{
			IP = ip,
			Reason = reason,
			BannedBy = bannedBy,
			BannedAt = DateTime.UtcNow
		};

		if (_bans.TryAdd(ip, entry))
		{
			_logger.LogInformation("IP banned: {IP} by {BannedBy} — {Reason}", ip, bannedBy, reason);
			return true;
		}
		return false;
	}

	public bool Unban(string ip)
	{
		ip = NormalizeIP(ip);
		if (_bans.TryRemove(ip, out var entry))
		{
			_logger.LogInformation("IP unbanned: {IP} (was banned by {BannedBy})", ip, entry.BannedBy);
			return true;
		}
		return false;
	}

	public IReadOnlyCollection<IPBanEntry> GetAllBans() => _bans.Values.ToList().AsReadOnly();

	private static string NormalizeIP(string ip) => ip.Trim();
}
