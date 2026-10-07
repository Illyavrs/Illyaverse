using System.Net.Http;
using System.Net.WebSockets;

namespace Illyaverse.Transfiguration;

internal static class NetworkPolicy
{
#if ILLYAVERSE_FIXTURE
    public const string Website = "http://127.0.0.1:18376";
    public const string Api = "http://127.0.0.1:18376";
    public const string Hubs = "http://127.0.0.1:18376";
#else
    public const string Website = "https://illya.test";
    public const string Api = "https://osu.illya.test";
    public const string Hubs = "https://spectator.illya.test";
#endif
    public const string ClientId = "illyaverse-local-client";
    public const string ClientSecret = "illyaverse-public-test-client";

    private static readonly HashSet<string> hosts = new(StringComparer.OrdinalIgnoreCase)
    {
#if ILLYAVERSE_FIXTURE
        "127.0.0.1",
#else
        "illya.test", "osu.illya.test", "spectator.illya.test", "a.illya.test", "b.illya.test",
        "api.illya.test", "bss.illya.test", "c.illya.test", "ce.illya.test", "c4.illya.test",
#endif
    };

    internal static bool Allows(Uri? uri, bool socket = false)
    {
        if (uri is null || !uri.IsAbsoluteUri || uri.UserInfo.Length != 0 || uri.Fragment.Length != 0) return false;
#if ILLYAVERSE_FIXTURE
        return hosts.Contains(uri.Host) && uri.Port == 18376 && uri.Scheme == (socket ? "ws" : "http");
#else
        return hosts.Contains(uri.Host) && uri.Port == 443 && uri.Scheme == (socket ? "wss" : "https");
#endif
    }

    internal static void Require(Uri? uri, bool socket = false)
    {
        if (!Allows(uri, socket)) throw new HttpRequestException("transfiguration_destination_blocked");
    }

    // ClientWebSocket uses SocketsHttpHandler with the original ws/wss request URI.
    internal static void HttpSendPrefix(SocketsHttpHandler __instance, HttpRequestMessage request)
    {
        if (__instance.AllowAutoRedirect) throw new HttpRequestException("transfiguration_redirect_guard_invalid");
        Require(request.RequestUri, socket: request.RequestUri?.Scheme is "ws" or "wss");
    }
    internal static void SocketConnectPrefix(Uri uri) => Require(uri, socket: true);
    internal static void SocketsHandlerPostfix(SocketsHttpHandler __instance) => __instance.AllowAutoRedirect = false;
    internal static void HttpHandlerPostfix(HttpClientHandler __instance) => __instance.AllowAutoRedirect = false;

    internal static bool ResourcePrefix(string url, ref string __result)
    {
        __result = Uri.TryCreate(url, UriKind.Absolute, out var uri) && Allows(uri) ? url : string.Empty;
        return false;
    }
}
