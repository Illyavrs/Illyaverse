using System.Net;
using System.Net.Http;
using System.Net.WebSockets;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging;

internal static class Program
{
    private static int checks;
    private static int fixtureHits;
    private static int forbiddenHits;

    private static void require(bool value, string name)
    {
        if (!value) throw new InvalidOperationException("FAIL " + name);
        checks++;
        Console.WriteLine("PASS " + name);
    }

    private static MethodInfo method(Type type, string name) => type.GetMethod(name, BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)!
        ?? throw new MissingMethodException(type.FullName, name);

    private static object? property(object value, string name) => value.GetType().GetProperty(name)!.GetValue(value);

    private static async Task blocked(Func<Task> action, string name)
    {
        try { await action(); }
        catch (HttpRequestException ex) when (ex.Message == "transfiguration_destination_blocked") { require(true, name); return; }
        throw new InvalidOperationException("FAIL request was not blocked: " + name);
    }

    public static async Task<int> Main(string[] args)
    {
        try { return await run(args); }
        catch (Exception ex) { Console.Error.WriteLine(ex); return 1; }
    }

    private static async Task<int> run(string[] args)
    {
        if (args is ["--policy", var productionPath]) return policy(productionPath);
        if (args is ["--reject", var fixturePath, var installation, var rejectionData, var scenario]) return rejection(fixturePath, installation, rejectionData, scenario);
        var nativeHosting = Environment.GetEnvironmentVariable("ILLYAVERSE_TRANSFIGURATION_NATIVE_PROOF") == "1";
        if (!nativeHosting && args.Length != 3) throw new ArgumentException("Arguments: fixture bridge DLL, native client directory, isolated data-transfiguration-proof directory");
        var bridgePath = Path.GetFullPath(nativeHosting ? Environment.GetEnvironmentVariable("ILLYAVERSE_TRANSFIGURATION_PROOF_BRIDGE")! : args[0]);
        var nativeRoot = Path.GetFullPath(nativeHosting ? Environment.GetEnvironmentVariable("ILLYAVERSE_LAZER_INSTALL_ROOT")! : args[1]);
        var dataRoot = Path.GetFullPath(nativeHosting ? Environment.GetEnvironmentVariable("ILLYAVERSE_LAZER_DATA_ROOT")! : args[2]);
        Directory.CreateDirectory(dataRoot);
        if (!Path.GetFileName(dataRoot).StartsWith("data-transfiguration-proof", StringComparison.Ordinal)) throw new ArgumentException("Proof data directory required");
        var session = nativeHosting ? Environment.GetEnvironmentVariable("ILLYAVERSE_LAZER_SESSION")!
            : Convert.ToHexString(RandomNumberGenerator.GetBytes(32)).ToLowerInvariant();
        var ready = Path.Combine(dataRoot, $"session-ready-{session}.json");
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_INSTALL_ROOT", nativeRoot);
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_DATA_ROOT", dataRoot);
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_SESSION", session);
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_READY_FILE", ready);
        if (nativeHosting)
        {
            require(File.Exists(ready), "mandatory native startup bridge completed before fixture Main");
            Console.WriteLine("NATIVE_PROOF_MAIN_ENTERED_AFTER_GUARD");
        }

        // The test fixture starts before installation of the client-only destination guard.
        var builder = WebApplication.CreateSlimBuilder();
        builder.Logging.ClearProviders();
        builder.WebHost.UseUrls("http://127.0.0.1:18376", "http://127.0.0.1:18377");
        await using var server = builder.Build();
        server.UseWebSockets();
        server.Run(async context =>
        {
            if (context.Connection.LocalPort == 18377) { Interlocked.Increment(ref forbiddenHits); await context.Response.WriteAsync("forbidden"); return; }
            Interlocked.Increment(ref fixtureHits);
            if (context.Request.Path == "/redirect") { context.Response.StatusCode = 302; context.Response.Headers.Location = "http://127.0.0.1:18377/leak"; return; }
            if (context.Request.Path == "/socket" && context.WebSockets.IsWebSocketRequest)
            {
                using var socket = await context.WebSockets.AcceptWebSocketAsync();
                await socket.SendAsync("fixture"u8.ToArray(), WebSocketMessageType.Text, true, CancellationToken.None);
                await socket.CloseOutputAsync(WebSocketCloseStatus.NormalClosure, "done", CancellationToken.None);
                return;
            }
            await context.Response.WriteAsync("fixture");
        });
        await server.StartAsync();
        try
        {
            var bridge = AssemblyLoadContext.Default.LoadFromAssemblyPath(bridgePath);
            require(bridge.GetManifestResourceNames().Contains("Illyaverse.Transfiguration.0Harmony.dll"), "Harmony is embedded in the single bridge DLL");
            method(bridge.GetType("StartupHook", true)!, "Initialize").Invoke(null, null);
            require(File.Exists(ready), "guard publishes readiness after all patches");
            using var state = JsonDocument.Parse(await File.ReadAllTextAsync(ready));
            require(state.RootElement.GetProperty("fixture").GetBoolean(), "loopback fixture is a distinct compile-time artifact");
            require(state.RootElement.GetProperty("session").GetString() == session, "readiness binds the launch nonce");
            require(state.RootElement.GetProperty("status").GetString() == "ready"
                && state.RootElement.GetProperty("endpoint").GetString() == "http://127.0.0.1:18376"
                && state.RootElement.GetProperty("protocol").GetInt32() == 1
                && state.RootElement.EnumerateObject().Count() == 5, "readiness uses exactly the negotiated five-field schema");
            require(Environment.GetEnvironmentVariable("OSU_DISABLE_ERROR_REPORTING") == "1", "official error reporting disabled before Main");
            require(Environment.GetEnvironmentVariable("OSU_EXTERNAL_UPDATE_PROVIDER") == "Illyaverse", "official updater and updater restart disabled before Main");

            var game = AssemblyLoadContext.Default.Assemblies.Single(a => a.GetName().Name == "osu.Game");
            var framework = AssemblyLoadContext.Default.Assemblies.Single(a => a.GetName().Name == "osu.Framework");
            var baseType = game.GetType("osu.Game.OsuGameBase", true)!;
            // No constructor, client Main, GameHost, Realm, APIAccess or cached credentials are executed/read.
            var gameShell = RuntimeHelpers.GetUninitializedObject(baseType);
            var endpoints = baseType.GetMethod("CreateEndpoints")!.Invoke(gameShell, null)!;
            require(property(endpoints, "APIUrl") as string == "http://127.0.0.1:18376", "real native CreateEndpoints returns the fixed fixture API");
            require(property(endpoints, "WebsiteUrl") as string == "http://127.0.0.1:18376", "real native CreateEndpoints returns the fixed fixture website");
            require(property(endpoints, "SpectatorUrl") as string == "http://127.0.0.1:18376/spectator", "real native CreateEndpoints redirects spectator");
            require(property(endpoints, "MultiplayerUrl") as string == "http://127.0.0.1:18376/multiplayer", "real native CreateEndpoints redirects multiplayer");
            require(property(endpoints, "MetadataUrl") as string == "http://127.0.0.1:18376/metadata", "real native CreateEndpoints redirects metadata");
            require(property(endpoints, "BeatmapSubmissionServiceUrl") is null && property(endpoints, "LivenessProbeUrl") is null, "unsupported BSS and official liveness probe are explicitly disabled");
            require(property(endpoints, "APIClientID") as string == "illyaverse-local-client", "OAuth client identity is fixed");

            var guard = bridge.GetType("Illyaverse.Transfiguration.Guard", true)!;
            require(((System.Collections.ICollection)guard.GetField("patched", BindingFlags.Static | BindingFlags.NonPublic)!.GetValue(null)!).Count == 17, "installed guard registry includes every exact destination and storage contract");
            var options = Activator.CreateInstance(framework.GetType("osu.Framework.HostOptions", true)!)!;
            options.GetType().GetProperty("PortableInstallation")!.SetValue(options, true);
            object?[] hostArgs = ["osu", options];
            method(guard, "HostPrefix").Invoke(null, hostArgs);
            require(hostArgs[0] as string == "illyaverse" && property(options, "IPCPipeName") as string == "illyaverse-lazer", "private game identity and IPC are independent of official lazer");
            require(property(options, "PortableInstallation") is false, "portable selection cannot override the private profile");
            object?[] storageArgs = [new FakeStorageHost(), null];
            require(method(guard, "StoragePrefix").Invoke(null, storageArgs) is false && storageArgs[1] as string == dataRoot, "storage override uses only the validated private data root");
            require(method(guard, "RulesetAssemblyPrefix").Invoke(null, [bridge]) is false, "connection sentinel does not add a playable ruleset");
            var storageType = game.GetType("osu.Game.IO.OsuStorage", true)!;
            var storageShell = RuntimeHelpers.GetUninitializedObject(storageType);
            require(storageType.GetProperty("CustomStoragePath")!.GetValue(storageShell) is null, "private storage.ini cannot redirect account storage to an official profile");
            object?[] customArgs = [null];
            require(storageType.GetMethod("TryChangeToCustomStorage")!.Invoke(storageShell, customArgs) is false
                && customArgs[0]?.ToString() == "NotAccessible", "explicit custom profile changes stop before Host.GetStorage");
            require(baseType.GetMethod("MigrateUserData")!.Invoke(gameShell, ["C:/fixture-official-profile-not-accessed"]) is false,
                "interactive profile migration stops without reading or moving any target");
            object?[] dataPathArgs = ["C:/fixture-official-profile-not-accessed"];
            method(guard, "FixedDataPathPrefix").Invoke(null, dataPathArgs);
            require(dataPathArgs[0] as string == string.Empty, "saved custom data path always remains empty");
            var desktop = AssemblyLoadContext.Default.Assemblies.Single(a => a.GetName().Name == "osu!");
            var desktopType = desktop.GetType("osu.Desktop.OsuGameDesktop", true)!;
            var desktopShell = RuntimeHelpers.GetUninitializedObject(desktopType);
            require(desktopType.GetMethod("RestartAppWhenExited")!.Invoke(desktopShell, null) is false,
                "stock client cannot restart itself outside the launcher under its fixed update-provider environment");
            require(desktopType.GetMethod("GetStorageForStableInstall")!.Invoke(desktopShell, null) is null,
                "stock client does not inspect any stable installation or cached stable account");

            using var handler = new SocketsHttpHandler();
            require(!handler.AllowAutoRedirect, "SocketsHttpHandler automatic redirects disabled");
            using var secondary = new HttpClientHandler();
            require(!secondary.AllowAutoRedirect, "HttpClientHandler automatic redirects disabled");
            using var client = new HttpClient(handler);
            require(await client.GetStringAsync("http://127.0.0.1:18376/ok") == "fixture", "allowed plain headless HTTP succeeds");
            using var redirect = await client.GetAsync("http://127.0.0.1:18376/redirect");
            require(redirect.StatusCode == HttpStatusCode.Found && forbiddenHits == 0, "foreign redirect is returned without forwarding traffic");
            await blocked(async () => { using var response = await client.GetAsync("https://osu.ppy.sh/oauth/token"); }, "official OAuth HTTP is blocked before network I/O");
            await blocked(async () => { using var response = await client.GetAsync("http://127.0.0.1:18377/leak"); }, "another loopback destination is blocked");
            await blocked(async () => { using var response = await client.GetAsync("http://user:secret@127.0.0.1:18376/leak"); }, "userinfo URLs are blocked");
            using var invoker = new HttpMessageInvoker(new SocketsHttpHandler());
            await blocked(async () => { using var response = await invoker.SendAsync(new HttpRequestMessage(HttpMethod.Get, "https://spectator.osu.ppy.sh/negotiate"), CancellationToken.None); }, "direct HttpMessageInvoker official spectator request is blocked");
            requireThrows(() => invoker.Send(new HttpRequestMessage(HttpMethod.Get, "https://osu.ppy.sh/oauth/token"), CancellationToken.None), "synchronous official HTTP is blocked before network I/O");
            using var misconfigured = new SocketsHttpHandler { AllowAutoRedirect = true };
            using var misconfiguredClient = new HttpClient(misconfigured);
            try { using var response = await misconfiguredClient.GetAsync("http://127.0.0.1:18376/redirect"); throw new InvalidOperationException("FAIL redirect guard was disabled"); }
            catch (HttpRequestException ex) when (ex.Message == "transfiguration_redirect_guard_invalid") { require(true, "re-enabling automatic redirects fails closed before network I/O"); }
            using var socket = new ClientWebSocket();
            await blocked(() => socket.ConnectAsync(new Uri("wss://osu.ppy.sh/socket"), CancellationToken.None), "official websocket is blocked before connecting");
            using var allowedSocket = new ClientWebSocket();
            await allowedSocket.ConnectAsync(new Uri("ws://127.0.0.1:18376/socket"), CancellationToken.None);
            var bytes = new byte[16];
            var received = await allowedSocket.ReceiveAsync(bytes, CancellationToken.None);
            require(Encoding.UTF8.GetString(bytes, 0, received.Count) == "fixture", "allowed plain headless websocket succeeds");
            require(forbiddenHits == 0 && fixtureHits == 3, "only the fixed local fixture received traffic");
            Console.WriteLine($"Native contracts: osu.Game {game.GetName().Version}; osu.Framework {framework.GetName().Version}");
            Console.WriteLine($"Native directory: {nativeRoot}; runtime {Environment.Version}");
            foreach (var assembly in new[] { game, framework, AssemblyLoadContext.Default.Assemblies.Single(a => a.GetName().Name == "osu!"), typeof(SocketsHttpHandler).Assembly })
                Console.WriteLine($"Native SHA256 {assembly.GetName().Name}: {Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(assembly.Location)))}");
            Console.WriteLine($"PASS {checks} isolated bridge checks; no GUI/client Main/GameHost/APIAccess/official request executed");
            return 0;
        }
        finally
        {
            await server.StopAsync();
            File.Delete(ready);
        }
    }

    private static void requireThrows(Func<HttpResponseMessage> action, string name)
    {
        try { using var response = action(); }
        catch (HttpRequestException ex) when (ex.Message == "transfiguration_destination_blocked") { require(true, name); return; }
        throw new InvalidOperationException("FAIL request was not blocked: " + name);
    }

    private static int policy(string productionPath)
    {
        // No startup hook, game assembly, private profile or network operation is used here.
        var bridge = AssemblyLoadContext.Default.LoadFromAssemblyPath(Path.GetFullPath(productionPath));
        var policy = bridge.GetType("Illyaverse.Transfiguration.NetworkPolicy", true)!;
        require(policy.GetField("Website")!.GetRawConstantValue() as string == "https://illya.test", "production website is fixed at build time");
        require(policy.GetField("Api")!.GetRawConstantValue() as string == "https://osu.illya.test", "production API is fixed at build time");
        require(policy.GetField("Hubs")!.GetRawConstantValue() as string == "https://spectator.illya.test", "production hubs are fixed at build time");
        var allows = method(policy, "Allows");
        foreach (var host in new[] { "illya.test", "osu.illya.test", "spectator.illya.test", "a.illya.test", "b.illya.test", "api.illya.test", "bss.illya.test", "c.illya.test", "ce.illya.test", "c4.illya.test" })
        {
            require(allows.Invoke(null, [new Uri($"https://{host}/fixture"), false]) is true, "fixed production HTTPS destination " + host);
            require(allows.Invoke(null, [new Uri($"wss://{host}/fixture"), true]) is true, "fixed production WSS destination " + host);
        }
        foreach (var url in new[] { "https://osu.ppy.sh/oauth/token", "https://spectator.osu.ppy.sh", "https://a.ppy.sh", "https://ilya.test", "https://illya.test.evil.invalid", "https://evilillya.test", "https://illya.test:444", "http://illya.test", "https://user:secret@illya.test", "https://illya.test/#fragment", "http://127.0.0.1:18376" })
            require(allows.Invoke(null, [new Uri(url), false]) is false, "production rejects " + new Uri(url).Host + " (" + new Uri(url).Scheme + ")");
        require(allows.Invoke(null, [new Uri("ws://illya.test/socket"), true]) is false, "production rejects insecure websocket");
        require(allows.Invoke(null, [new Uri("wss://osu.ppy.sh/socket"), true]) is false, "production rejects official websocket");
        Console.WriteLine($"PASS {checks} production policy checks; no profile/client/network operation executed");
        return 0;
    }

    private static int rejection(string fixturePath, string installation, string rejectionData, string scenario)
    {
        var data = Path.GetFullPath(rejectionData);
        if (!Path.GetFileName(data).StartsWith("data-transfiguration-proof", StringComparison.Ordinal)) throw new ArgumentException("Proof data directory required");
        Directory.CreateDirectory(data);
        var session = Convert.ToHexString(RandomNumberGenerator.GetBytes(32)).ToLowerInvariant();
        var ready = Path.Combine(data, $"session-ready-{session}.json");
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_INSTALL_ROOT", Path.GetFullPath(installation));
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_DATA_ROOT", data);
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_SESSION", scenario == "nonce" ? "invalid" : session);
        Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_READY_FILE", scenario == "ready-path" ? Path.Combine(data, "foreign-ready.json") : ready);
        if (scenario == "relative") Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_DATA_ROOT", "relative-data");
        if (scenario == "unc") Environment.SetEnvironmentVariable("ILLYAVERSE_LAZER_DATA_ROOT", @"\\fixture.invalid\never-connected");
        if (scenario == "ready-exists") File.WriteAllText(ready, "fixture-owner-marker");
        var expected = scenario switch
        {
            "nonce" => "transfiguration_session_invalid",
            "ready-path" => "transfiguration_ready_path_invalid",
            "ready-exists" => "transfiguration_ready_file_exists",
            "relative" => "transfiguration_path_not_absolute",
            "unc" => "transfiguration_path_not_local",
            _ => throw new ArgumentException("Unknown rejection scenario")
        };
        try
        {
            var bridge = AssemblyLoadContext.Default.LoadFromAssemblyPath(Path.GetFullPath(fixturePath));
            try { method(bridge.GetType("StartupHook", true)!, "Initialize").Invoke(null, null); }
            catch (TargetInvocationException ex) when (ex.InnerException is InvalidOperationException cause && cause.Message == expected)
            {
                require(scenario == "ready-exists" ? File.ReadAllText(ready) == "fixture-owner-marker" : !File.Exists(ready), "invalid " + scenario + " fails without overwriting or creating readiness");
                require(!AssemblyLoadContext.Default.Assemblies.Any(a => a.GetName().Name == "osu.Game"), "invalid " + scenario + " stops before loading game or cached account storage");
                return 0;
            }
            throw new InvalidOperationException("FAIL rejection scenario " + scenario);
        }
        finally { if (scenario == "ready-exists") File.Delete(ready); }
    }

    private sealed class FakeStorageHost
    {
        public object GetStorage(string path) => path;
    }
}
