using System.Net.Http;
using System.Net.WebSockets;
using System.Reflection;
using System.Runtime.Loader;
using System.Security.Cryptography;
using System.Text.Json;
using HarmonyLib;

namespace Illyaverse.Transfiguration;

internal static class Guard
{
    private const string owner = "community.illyaverse.transfiguration";
    private static readonly object gate = new();
    private static string installRoot = string.Empty;
    private static string dataRoot = string.Empty;
    private static readonly List<MethodBase> patched = new();
    private static Assembly game = null!;
    private static Assembly framework = null!;
    private static Assembly desktop = null!;
    private static Harmony harmony = null!;

    internal static void Initialize()
    {
        lock (gate)
        {
            if (LaunchState.Active) return;
            var session = required("ILLYAVERSE_LAZER_SESSION");
            if (session.Length != 64 || !session.All(Uri.IsHexDigit)) fail("session_invalid");
            installRoot = absoluteDirectory(required("ILLYAVERSE_LAZER_INSTALL_ROOT"));
            dataRoot = absoluteDirectory(required("ILLYAVERSE_LAZER_DATA_ROOT"));
            validateDataRoot();
            var readyFile = canonicalPath(required("ILLYAVERSE_LAZER_READY_FILE"));
            if (!string.Equals(readyFile, Path.Combine(dataRoot, $"session-ready-{session}.json"), StringComparison.OrdinalIgnoreCase)) fail("ready_path_invalid");
            if (File.Exists(readyFile)) fail("ready_file_exists");
            rejectReparse(dataRoot);
            Environment.SetEnvironmentVariable("OSU_DISABLE_ERROR_REPORTING", "1");
            Environment.SetEnvironmentVariable("OSU_EXTERNAL_UPDATE_PROVIDER", "Illyaverse");
            // No inherited hook may apply unexpectedly to child processes. The active runtime property is unchanged.
            Environment.SetEnvironmentVariable("DOTNET_STARTUP_HOOKS", null);
            AssemblyLoadContext.Default.Resolving += resolveClientAssembly;
            framework = loadClient("osu.Framework.dll");
            game = loadClient("osu.Game.dll");
            desktop = loadClient("osu!.dll");
            harmony = new Harmony(owner);
            verifyContracts();
            installNetworkGuards();
            installClientGuards();
            foreach (var method in patched)
                if (Harmony.GetPatchInfo(method)?.Owners.Contains(owner) != true) fail("patch_not_installed");
            LaunchState.Active = true;
            using var readyStream = new FileStream(readyFile, FileMode.CreateNew, FileAccess.Write, FileShare.Read);
            JsonSerializer.Serialize(readyStream, new
            {
                protocol = 1,
                session,
                status = "ready",
                endpoint = NetworkPolicy.Website,
#if ILLYAVERSE_FIXTURE
                fixture = true,
#else
                fixture = false,
#endif
            });
            readyStream.Flush(flushToDisk: true);
        }
    }

    private static string required(string key) => Environment.GetEnvironmentVariable(key) is { Length: > 0 } value
        ? value : throw new InvalidOperationException($"transfiguration_{key.ToLowerInvariant()}_missing");

    private static string absoluteDirectory(string value)
    {
        if (!Path.IsPathFullyQualified(value)) fail("path_not_absolute");
        var full = Path.TrimEndingDirectorySeparator(canonicalPath(value));
        if (!Directory.Exists(full)) fail("directory_missing");
        return full;
    }

    private static string canonicalPath(string value)
    {
        if (!Path.IsPathFullyQualified(value)) fail("path_not_absolute");
        var path = Path.GetFullPath(value);
        if (path.StartsWith(@"\\?\", StringComparison.Ordinal))
        {
            if (path.Length < 7 || !char.IsAsciiLetter(path[4]) || path[5] != ':' || path[6] != '\\') fail("path_not_local");
            path = path[4..];
        }
        if (path.StartsWith(@"\\", StringComparison.Ordinal)) fail("path_not_local");
        return path;
    }

    private static void validateDataRoot()
    {
#if ILLYAVERSE_FIXTURE
        if (!Path.GetFileName(dataRoot).StartsWith("data-transfiguration-proof", StringComparison.Ordinal)) fail("fixture_data_path_invalid");
#else
        var expected = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Illyaverse", "lazer");
        if (!string.Equals(dataRoot, expected, StringComparison.OrdinalIgnoreCase)) fail("data_path_invalid");
#endif
        if (string.Equals(dataRoot, installRoot, StringComparison.OrdinalIgnoreCase)
            || dataRoot.StartsWith(installRoot + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase)) fail("data_install_overlap");
    }

    private static void rejectReparse(string path)
    {
        if ((File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0) fail("data_reparse_point");
        var parent = Directory.GetParent(path);
        if (parent is not null && parent.Name.Equals("Illyaverse", StringComparison.OrdinalIgnoreCase)
            && (parent.Attributes & FileAttributes.ReparsePoint) != 0) fail("data_reparse_point");
    }

    private static Assembly loadClient(string filename)
    {
        var path = Path.Combine(installRoot, filename);
        if (!File.Exists(path)) fail("client_assembly_missing");
        var expectedName = Path.GetFileNameWithoutExtension(filename);
        var loaded = AssemblyLoadContext.Default.Assemblies.FirstOrDefault(a => a.GetName().Name == expectedName);
        if (loaded is not null)
        {
            if (!string.Equals(Path.GetFullPath(loaded.Location), path, StringComparison.OrdinalIgnoreCase)) fail("client_assembly_location_mismatch");
            return loaded;
        }
        return AssemblyLoadContext.Default.LoadFromAssemblyPath(path);
    }

    private static Assembly? resolveClientAssembly(AssemblyLoadContext context, AssemblyName name)
    {
        if (name.Name is null || name.Name.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0) return null;
        var path = Path.Combine(installRoot, name.Name + ".dll");
        return File.Exists(path) ? context.LoadFromAssemblyPath(path) : null;
    }

    private static Type type(Assembly assembly, string name) => assembly.GetType(name, throwOnError: true)!;
    private static MethodInfo method(Type t, string name, params Type[] parameters) => t.GetMethod(name,
        BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static,
        binder: null, types: parameters, modifiers: null) ?? throw new MissingMethodException(t.FullName, name);
    private static MethodInfo local(Type t, string name) => t.GetMethod(name, BindingFlags.NonPublic | BindingFlags.Static)
        ?? throw new MissingMethodException(t.FullName, name);

    private static void patch(MethodBase target, string patchName, Type? patchType = null, bool postfix = false)
    {
        var injection = new HarmonyMethod(local(patchType ?? typeof(Guard), patchName)) { priority = Priority.First };
        harmony.Patch(target, prefix: postfix ? null : injection, postfix: postfix ? injection : null);
        patched.Add(target);
    }

    private static void verifyContracts()
    {
        // This allowlist is updated only after the isolated contract and transport proofs pass.
        requireAssembly(game, "2026.1005.1.0", "AD9527E411111CB4B4045C76DF61C3474FD872A04AF73160C05D24E44A588249");
        requireAssembly(framework, "2026.1005.0.0", "93B62F61C72539ED6FDB3C8EBA18D276A74DE8E1C094280B2531F5A76CAEA528");
        requireAssembly(desktop, "2026.1005.1.0", "18AE9BB6BB441E9A654AC72C859F527041BDE52E5DCE4109F8D9074584FA57A4");
        requireAssembly(typeof(SocketsHttpHandler).Assembly, "10.0.0.0", "B98ED0CB80DD54C33FC049F33AF5448710752155BBD5CC8CBCD932E01F066801");
        requireAssembly(typeof(ClientWebSocket).Assembly, "10.0.0.0", "F308A69B9EE1AC982F53D1D94374F4972A7E3C700742901FF7C2BBFB570019D2");
        if (Environment.Version != new Version(10, 0, 12)) fail("runtime_version_unverified");
        var apiVersion = type(game, "osu.Game.Rulesets.Ruleset").GetField("CURRENT_RULESET_API_VERSION", BindingFlags.Public | BindingFlags.Static)?.GetRawConstantValue();
        if (!Equals(apiVersion, "2026.818.0")) fail("ruleset_api_incompatible");
        var endpoint = type(game, "osu.Game.Online.EndpointConfiguration");
        foreach (var property in new[] { "APIUrl", "WebsiteUrl", "APIClientID", "APIClientSecret", "SpectatorUrl", "MultiplayerUrl", "MetadataUrl", "BeatmapSubmissionServiceUrl", "LivenessProbeUrl" })
        {
            var value = endpoint.GetProperty(property);
            if (value?.PropertyType != typeof(string) || value.SetMethod?.IsPublic != true) fail("endpoint_contract_incompatible");
        }
        if (method(type(game, "osu.Game.OsuGameBase"), "CreateEndpoints").ReturnType != endpoint) fail("endpoint_factory_incompatible");
        _ = method(type(framework, "osu.Framework.Host"), "GetSuitableDesktopHost", typeof(string), type(framework, "osu.Framework.HostOptions"));
        _ = method(type(framework, "osu.Framework.Platform.DesktopGameHost"), "GetDefaultGameStorage");
        _ = method(type(desktop, "osu.Desktop.OsuGameDesktop"), "GetStorageForStableInstall");
        _ = typeof(osu.Game.Rulesets.Transfiguration.BridgeRuleset).GetMethods();
    }

    private static void requireAssembly(Assembly assembly, string version, string sha256)
    {
        if (assembly.GetName().Version?.ToString() != version) fail("client_version_unverified");
        using var stream = File.OpenRead(assembly.Location);
        if (Convert.ToHexString(SHA256.HashData(stream)) != sha256) fail("client_assembly_unverified");
    }

    private static void installNetworkGuards()
    {
        patch(typeof(SocketsHttpHandler).GetConstructor(Type.EmptyTypes)!, "SocketsHandlerPostfix", typeof(NetworkPolicy), postfix: true);
        patch(typeof(HttpClientHandler).GetConstructor(Type.EmptyTypes)!, "HttpHandlerPostfix", typeof(NetworkPolicy), postfix: true);
        patch(method(typeof(SocketsHttpHandler), "Send", typeof(HttpRequestMessage), typeof(CancellationToken)), "HttpSendPrefix", typeof(NetworkPolicy));
        patch(method(typeof(SocketsHttpHandler), "SendAsync", typeof(HttpRequestMessage), typeof(CancellationToken)), "HttpSendPrefix", typeof(NetworkPolicy));
        patch(method(typeof(ClientWebSocket), "ConnectAsync", typeof(Uri), typeof(CancellationToken)), "SocketConnectPrefix", typeof(NetworkPolicy));
        patch(method(typeof(ClientWebSocket), "ConnectAsync", typeof(Uri), typeof(HttpMessageInvoker), typeof(CancellationToken)), "SocketConnectPrefix", typeof(NetworkPolicy));
        patch(method(type(game, "osu.Game.Online.TrustedDomainOnlineStore"), "GetLookupUrl", typeof(string)), "ResourcePrefix", typeof(NetworkPolicy));
    }

    private static void installClientGuards()
    {
        patch(method(type(game, "osu.Game.OsuGameBase"), "CreateEndpoints"), "EndpointsPrefix");
        patch(method(type(framework, "osu.Framework.Host"), "GetSuitableDesktopHost", typeof(string), type(framework, "osu.Framework.HostOptions")), "HostPrefix");
        patch(method(type(framework, "osu.Framework.Platform.DesktopGameHost"), "GetDefaultGameStorage"), "StoragePrefix");
        patch(method(type(desktop, "osu.Desktop.OsuGameDesktop"), "GetStorageForStableInstall"), "NoStablePrefix");
        patch(method(type(game, "osu.Game.Rulesets.RulesetStore"), "addRuleset", typeof(Assembly)), "RulesetAssemblyPrefix");
        patch(method(type(framework, "osu.Framework.Platform.DesktopGameHost"), "OpenUrlExternally", typeof(string)), "ExternalUrlPrefix");
        var storage = type(game, "osu.Game.IO.OsuStorage");
        patch(storage.GetProperty("CustomStoragePath")!.GetMethod!, "NoCustomPathPrefix");
        patch(method(storage, "TryChangeToCustomStorage", typeof(osu.Game.IO.OsuStorageError).MakeByRefType()), "NoCustomStoragePrefix");
        patch(method(storage, "ChangeDataPath", typeof(string)), "FixedDataPathPrefix");
        patch(method(type(game, "osu.Game.OsuGameBase"), "MigrateUserData", typeof(string)), "NoMigrationPrefix");
    }

    private static bool EndpointsPrefix(ref object __result)
    {
        var endpoint = Activator.CreateInstance(type(game, "osu.Game.Online.EndpointConfiguration"))!;
        set(endpoint, "APIUrl", NetworkPolicy.Api);
        set(endpoint, "WebsiteUrl", NetworkPolicy.Website);
        set(endpoint, "APIClientID", NetworkPolicy.ClientId);
        set(endpoint, "APIClientSecret", NetworkPolicy.ClientSecret);
        set(endpoint, "SpectatorUrl", NetworkPolicy.Hubs + "/spectator");
        set(endpoint, "MultiplayerUrl", NetworkPolicy.Hubs + "/multiplayer");
        set(endpoint, "MetadataUrl", NetworkPolicy.Hubs + "/metadata");
        set(endpoint, "BeatmapSubmissionServiceUrl", null);
        set(endpoint, "LivenessProbeUrl", null);
        __result = endpoint;
        return false;
    }

    private static void set(object target, string name, object? value) => target.GetType().GetProperty(name)!.SetValue(target, value);

    private static void HostPrefix(ref string gameName, ref object hostOptions)
    {
        gameName = "illyaverse";
        hostOptions ??= Activator.CreateInstance(type(framework, "osu.Framework.HostOptions"))!;
        set(hostOptions, "IPCPipeName", "illyaverse-lazer");
        set(hostOptions, "FriendlyGameName", "Illyaverse • osu!lazer");
        set(hostOptions, "PortableInstallation", false);
    }

    private static bool StoragePrefix(object __instance, ref object __result)
    {
        __result = method(__instance.GetType(), "GetStorage", typeof(string)).Invoke(__instance, [dataRoot])!;
        return false;
    }

    private static bool NoStablePrefix(ref object? __result) { __result = null; return false; }
    private static bool NoCustomPathPrefix(ref string? __result) { __result = null; return false; }
    private static bool NoCustomStoragePrefix(ref osu.Game.IO.OsuStorageError error, ref bool __result)
    {
        error = osu.Game.IO.OsuStorageError.NotAccessible;
        __result = false;
        return false;
    }
    private static void FixedDataPathPrefix(ref string newPath) => newPath = string.Empty;
    private static bool NoMigrationPrefix(ref bool __result) { __result = false; return false; }
    private static bool RulesetAssemblyPrefix(Assembly assembly) => assembly != typeof(StartupHook).Assembly;
    private static void ExternalUrlPrefix(string url) => NetworkPolicy.Require(new Uri(url, UriKind.Absolute));
    private static void fail(string reason) => throw new InvalidOperationException("transfiguration_" + reason);
}
