using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Xml;

namespace Tempest.CLI.Server;

// ponytail: minimal UPnP IGD port mapper; no external dependency, AOT-safe (raw SOAP + XmlReader).
// Covers WANIPConnection/WANPPPConnection. Add NAT-PMP/PCP only if routers in the wild need it.
internal sealed class UpnpPortMapper(int port, string description) : IAsyncDisposable
{
    private readonly HttpClient _http = new();
    private string? _controlUrl;
    private string? _serviceType;

    public async Task MapAsync(CancellationToken cancellationToken = default)
    {
        var (location, deviceType) = await DiscoverAsync(cancellationToken).ConfigureAwait(false);
        if (location is null)
            throw new InvalidOperationException("No UPnP gateway responded to discovery.");

        var (controlUrl, serviceType) = await ResolveControlUrlAsync(location, cancellationToken).ConfigureAwait(false);
        if (controlUrl is null || serviceType is null)
            throw new InvalidOperationException("UPnP gateway does not expose a port mapping service.");

        _controlUrl = controlUrl;
        _serviceType = serviceType;

        var localIp = GetLocalIpForGateway(new Uri(controlUrl).Host);
        var body = BuildSoap("AddPortMapping", serviceType,
            $"<NewRemoteHost></NewRemoteHost>" +
            $"<NewExternalPort>{port}</NewExternalPort>" +
            $"<NewProtocol>TCP</NewProtocol>" +
            $"<NewInternalPort>{port}</NewInternalPort>" +
            $"<NewInternalClient>{localIp}</NewInternalClient>" +
            $"<NewEnabled>1</NewEnabled>" +
            $"<NewPortMappingDescription>{XmlEncode(description)}</NewPortMappingDescription>" +
            $"<NewLeaseDuration>0</NewLeaseDuration>");

        await SendSoapAsync(controlUrl, serviceType, "AddPortMapping", body, cancellationToken).ConfigureAwait(false);
    }

    public async Task UnmapAsync(CancellationToken cancellationToken = default)
    {
        if (_controlUrl is null || _serviceType is null)
            return;

        var body = BuildSoap("DeletePortMapping", _serviceType,
            "<NewRemoteHost></NewRemoteHost>" +
            $"<NewExternalPort>{port}</NewExternalPort>" +
            "<NewProtocol>TCP</NewProtocol>");

        await SendSoapAsync(_controlUrl, _serviceType, "DeletePortMapping", body, cancellationToken).ConfigureAwait(false);
    }

    public ValueTask DisposeAsync()
    {
        _http.Dispose();
        return ValueTask.CompletedTask;
    }

    private static async Task<(string? Location, string DeviceType)> DiscoverAsync(CancellationToken cancellationToken)
    {
        const string deviceType = "urn:schemas-upnp-org:device:InternetGatewayDevice:1";
        var message = Encoding.UTF8.GetBytes(
            "M-SEARCH * HTTP/1.1\r\n" +
            "HOST: 239.255.255.250:1900\r\n" +
            "MAN: \"ssdp:discover\"\r\n" +
            "MX: 2\r\n" +
            $"ST: {deviceType}\r\n" +
            "\r\n");

        using var udp = new UdpClient();
        udp.EnableBroadcast = true;
        var multicastEndpoint = new IPEndPoint(IPAddress.Parse("239.255.255.250"), 1900);
        await udp.SendAsync(message, message.Length, multicastEndpoint).ConfigureAwait(false);

        using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cts.Token, cancellationToken);

        while (!linked.Token.IsCancellationRequested)
        {
            try
            {
                var result = await udp.ReceiveAsync(linked.Token).ConfigureAwait(false);
                var text = Encoding.UTF8.GetString(result.Buffer);
                var location = ParseHeader(text, "LOCATION");
                if (!string.IsNullOrEmpty(location))
                    return (location, deviceType);
            }
            catch (OperationCanceledException)
            {
                break;
            }
            catch
            {
                // Ignore malformed responses and continue listening until timeout.
            }
        }

        return (null, deviceType);
    }

    private static string? ParseHeader(string text, string name)
    {
        foreach (var line in text.Split('\r', '\n'))
        {
            var idx = line.IndexOf(':');
            if (idx > 0 && line[..idx].Trim().Equals(name, StringComparison.OrdinalIgnoreCase))
                return line[(idx + 1)..].Trim();
        }
        return null;
    }

    private async Task<(string? ControlUrl, string? ServiceType)> ResolveControlUrlAsync(string location, CancellationToken cancellationToken)
    {
        using var response = await _http.GetAsync(location, cancellationToken).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        var xml = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);

        using var reader = XmlReader.Create(new StringReader(xml), new XmlReaderSettings { Async = true });
        string? urlBase = null;
        string? controlUrl = null;
        string? serviceType = null;
        string? currentServiceType = null;
        string? currentControlUrl = null;
        var inService = false;

        while (await reader.ReadAsync().ConfigureAwait(false))
        {
            if (reader.NodeType == XmlNodeType.Element)
            {
                var name = reader.Name;
                if (name.Equals("URLBase", StringComparison.OrdinalIgnoreCase))
                {
                    urlBase = (await reader.ReadElementContentAsStringAsync().ConfigureAwait(false)).Trim();
                }
                else if (name.Equals("service", StringComparison.OrdinalIgnoreCase))
                {
                    inService = true;
                    currentServiceType = null;
                    currentControlUrl = null;
                }
                else if (inService && name.Equals("serviceType", StringComparison.OrdinalIgnoreCase))
                {
                    currentServiceType = (await reader.ReadElementContentAsStringAsync().ConfigureAwait(false)).Trim();
                }
                else if (inService && name.Equals("controlURL", StringComparison.OrdinalIgnoreCase))
                {
                    currentControlUrl = (await reader.ReadElementContentAsStringAsync().ConfigureAwait(false)).Trim();
                }
            }
            else if (inService && reader.NodeType == XmlNodeType.EndElement && reader.Name.Equals("service", StringComparison.OrdinalIgnoreCase))
            {
                if (currentServiceType != null && currentControlUrl != null &&
                    (currentServiceType.Contains("WANIPConnection", StringComparison.OrdinalIgnoreCase) ||
                     currentServiceType.Contains("WANPPPConnection", StringComparison.OrdinalIgnoreCase)))
                {
                    controlUrl = currentControlUrl;
                    serviceType = currentServiceType;
                    break;
                }
                inService = false;
            }
        }

        if (string.IsNullOrEmpty(controlUrl))
            return (null, null);

        controlUrl = controlUrl.Trim();

        if (Uri.TryCreate(controlUrl, UriKind.Absolute, out var absoluteControlUrl) &&
            absoluteControlUrl.IsAbsoluteUri &&
            (absoluteControlUrl.Scheme == Uri.UriSchemeHttp || absoluteControlUrl.Scheme == Uri.UriSchemeHttps))
        {
            return (absoluteControlUrl.ToString(), serviceType);
        }

        var baseUri = string.IsNullOrEmpty(urlBase) ? location : urlBase;
        if (!Uri.TryCreate(baseUri, UriKind.Absolute, out var baseUriObj))
            baseUriObj = new Uri(location);

        if (Uri.TryCreate(baseUriObj, controlUrl, out var absolute) &&
            absolute.IsAbsoluteUri &&
            (absolute.Scheme == Uri.UriSchemeHttp || absolute.Scheme == Uri.UriSchemeHttps))
        {
            return (absolute.ToString(), serviceType);
        }

        return (null, null);
    }

    private static string GetLocalIpForGateway(string gatewayHost)
    {
        try
        {
            if (!IPAddress.TryParse(gatewayHost, out var gatewayIp))
            {
                gatewayIp = Dns.GetHostAddresses(gatewayHost)
                    .FirstOrDefault(static a => a.AddressFamily == AddressFamily.InterNetwork);
            }

            if (gatewayIp is null)
                return GetFirstLocalIPv4() ?? "127.0.0.1";

            using var socket = new Socket(AddressFamily.InterNetwork, SocketType.Dgram, ProtocolType.Udp);
            socket.Connect(new IPEndPoint(gatewayIp, 1900));
            return ((IPEndPoint)socket.LocalEndPoint!).Address.ToString();
        }
        catch
        {
            return GetFirstLocalIPv4() ?? "127.0.0.1";
        }
    }

    private static string? GetFirstLocalIPv4()
    {
        foreach (var ni in System.Net.NetworkInformation.NetworkInterface.GetAllNetworkInterfaces())
        {
            if (ni.OperationalStatus != System.Net.NetworkInformation.OperationalStatus.Up)
                continue;

            foreach (var addr in ni.GetIPProperties().UnicastAddresses)
            {
                if (addr.Address.AddressFamily == AddressFamily.InterNetwork && !IPAddress.IsLoopback(addr.Address))
                    return addr.Address.ToString();
            }
        }
        return null;
    }

    private async Task SendSoapAsync(string url, string serviceType, string action, string body, CancellationToken cancellationToken)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var requestUri) ||
            (requestUri.Scheme != Uri.UriSchemeHttp && requestUri.Scheme != Uri.UriSchemeHttps))
        {
            throw new InvalidOperationException($"UPnP control URL is not a valid absolute HTTP/HTTPS URI: '{url}'");
        }

        var content = new StringContent(body, Encoding.UTF8, "text/xml");
        content.Headers.TryAddWithoutValidation("SOAPACTION", $"\"{serviceType}#{action}\"");
        using var response = await _http.PostAsync(requestUri, content, cancellationToken).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    private static string BuildSoap(string action, string serviceType, string parameters)
    {
        return $"""<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{serviceType}">{parameters}</u:{action}></s:Body></s:Envelope>""";
    }

    private static string XmlEncode(string value)
    {
        return value.Replace("&", "&amp;").Replace("<", "&lt;").Replace(">", "&gt;").Replace("\"", "&quot;").Replace("'", "&apos;");
    }
}
