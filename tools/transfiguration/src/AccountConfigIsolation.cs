using System.Reflection;
using System.Runtime.CompilerServices;
using System.Diagnostics.CodeAnalysis;
using osu.Framework.Configuration;
using osu.Framework.Platform;
using osu.Game;
using osu.Game.Configuration;
using osu.Game.Online;
using osu.Game.Online.API;

namespace Illyaverse.Transfiguration;

// The shared library/Realm/files remain on stock Storage. Only game settings and
// credentials use another filename, selected before IniConfigManager.Load runs.
internal static class AccountConfigIsolation
{
    internal const string Filename = "game.illyaverse.ini";
    private const BindingFlags instance = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
    private static readonly ConditionalWeakTable<OsuConfigManager, ConfigState> configs = new();
    private static string libraryRoot = string.Empty;
    private static FieldInfo storageField = null!;
    internal static MethodInfo FilenameGetter { get; private set; } = null!;
    internal static ConstructorInfo ConfigConstructor { get; private set; } = null!;
    internal static ConstructorInfo ApiConstructor { get; private set; } = null!;
    private sealed class ConfigState { internal bool Loaded; }

    internal static void VerifyContracts(string effectiveLibraryRoot)
    {
        libraryRoot = effectiveLibraryRoot;
        var closedIni = typeof(IniConfigManager<OsuSetting>);
        if (typeof(OsuConfigManager).BaseType != closedIni) fail("config_base_incompatible");
        FilenameGetter = closedIni.GetProperty("Filename", instance)?.GetMethod!;
        storageField = closedIni.GetField("storage", instance)!;
        ConfigConstructor = typeof(OsuConfigManager).GetConstructor([typeof(Storage)])!;
        ApiConstructor = typeof(APIAccess).GetConstructor([typeof(OsuGameBase), typeof(OsuConfigManager), typeof(EndpointConfiguration), typeof(string)])!;
        if (FilenameGetter?.ReturnType != typeof(string) || !FilenameGetter.IsVirtual
            || FilenameGetter.DeclaringType != closedIni || FilenameGetter.GetParameters().Length != 0
            || storageField?.FieldType != typeof(Storage) || !storageField.IsInitOnly
            || ConfigConstructor is null || ApiConstructor is null) fail("config_contract_incompatible");
    }

    private static bool FilenamePrefix(object __instance, ref string __result)
    {
        // Generic methods may share native code. Leave all other config managers alone.
        if (__instance is not OsuConfigManager config) return true;
        requireExactManager(config);
        requirePrivateFile();
        __result = Filename;
        return false;
    }

    private static void ConfigConstructorPrefix(OsuConfigManager __instance, Storage storage)
    {
        requireExactManager(__instance);
        requireStorage(storage);
        requirePrivateFile();
        configs.Add(__instance, new ConfigState());
    }

    private static void ConfigConstructorPostfix(OsuConfigManager __instance)
    {
        requireStorage(storageField.GetValue(__instance) as Storage);
        if (!configs.TryGetValue(__instance, out var state)
            || FilenameGetter.Invoke(__instance, null) as string != Filename) fail("config_not_isolated");
        state.Loaded = true;
    }

    private static void ApiConstructorPrefix(OsuConfigManager config, EndpointConfiguration endpoints)
    {
        // Runs before APIAccess reads Token/Username or starts its background thread.
        requireExactManager(config);
        requireStorage(storageField.GetValue(config) as Storage);
        if (!configs.TryGetValue(config, out var state) || !state.Loaded
            || FilenameGetter.Invoke(config, null) as string != Filename) fail("config_not_isolated");
        if (endpoints.APIUrl != NetworkPolicy.Api || endpoints.WebsiteUrl != NetworkPolicy.Website
            || endpoints.APIClientID != NetworkPolicy.ClientId || endpoints.APIClientSecret != NetworkPolicy.ClientSecret)
            fail("api_config_not_private");
    }

    private static void requireExactManager(OsuConfigManager config)
    {
        if (config is null || config.GetType() != typeof(OsuConfigManager)) fail("config_manager_unverified");
    }

    private static void requireStorage(Storage? storage)
    {
        if (storage is null || !string.Equals(Path.TrimEndingDirectorySeparator(Path.GetFullPath(storage.GetFullPath("."))),
                libraryRoot, StringComparison.OrdinalIgnoreCase)) fail("config_storage_unverified");
    }

    private static void requirePrivateFile()
    {
        var path = Path.Combine(libraryRoot, Filename);
        try
        {
            // GetAttributes also rejects dangling links, for which File.Exists is false.
            var attributes = File.GetAttributes(path);
            if ((attributes & (FileAttributes.ReparsePoint | FileAttributes.Directory)) != 0)
                fail("config_file_unverified");
        }
        catch (FileNotFoundException) { }
        catch (DirectoryNotFoundException) { }
    }

    [DoesNotReturn]
    private static void fail(string reason) => throw new InvalidOperationException("transfiguration_" + reason);
}
