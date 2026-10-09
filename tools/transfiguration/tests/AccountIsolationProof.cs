using System.Reflection;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using System.Text;
using osu.Framework.Configuration;
using osu.Framework.Platform;
using osu.Game;
using osu.Game.Configuration;
using osu.Game.Online;
using osu.Game.Online.API;

internal static class AccountIsolationProof
{
    private const BindingFlags instance = BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public;
    private static int checks;
    private static int pausedApiThreads;
    private static readonly AutoResetEvent apiWorkerStopped = new(false);

    private static void require(bool value, string name)
    {
        if (!value) throw new InvalidOperationException("FAIL account isolation " + name);
        checks++;
        Console.WriteLine("PASS account isolation " + name);
    }

    private static void rejected(Action action, string reason, string name)
    {
        try { action(); }
        catch (InvalidOperationException ex) when (ex.Message == "transfiguration_" + reason) { require(true, name); return; }
        catch (TargetInvocationException ex) when (ex.InnerException is InvalidOperationException cause && cause.Message == "transfiguration_" + reason) { require(true, name); return; }
        throw new InvalidOperationException("FAIL account isolation expected rejection " + name);
    }

    // Test-only interception: the constructor's worker exits at its entry point,
    // before auth or network work. No production bridge uses this patch.
    private static bool pauseApiWorker()
    {
        Interlocked.Increment(ref pausedApiThreads);
        apiWorkerStopped.Set();
        return false;
    }

    internal static int Run(Assembly bridge, string root, object fixedEndpoints)
    {
        if (!Path.GetFileName(root).StartsWith("data-transfiguration-proof", StringComparison.Ordinal))
            throw new InvalidOperationException("Proof-owned storage is required");
        var officialPath = Path.Combine(root, "game.ini");
        var privatePath = Path.Combine(root, "game.illyaverse.ini");
        var realmPath = Path.Combine(root, "client.realm");
        var resourcePath = Path.Combine(root, "files", "proof-library-resource");
        Directory.CreateDirectory(Path.GetDirectoryName(resourcePath)!);
        File.WriteAllText(realmPath, "synthetic-realm-marker-not-a-real-database", Encoding.UTF8);
        File.WriteAllText(resourcePath, "synthetic-existing-beatmap-resource", Encoding.UTF8);
        var realmHash = SHA256.HashData(File.ReadAllBytes(realmPath));
        var resourceHash = SHA256.HashData(File.ReadAllBytes(resourcePath));
        var realmCreated = File.GetCreationTimeUtc(realmPath);
        var resourceCreated = File.GetCreationTimeUtc(resourcePath);
        var storage = new NativeStorage(root);
        var isolation = bridge.GetType("Illyaverse.Transfiguration.AccountConfigIsolation", true)!;
        var filename = typeof(IniConfigManager<OsuSetting>).GetProperty("Filename", instance)!.GetMethod!;
        var storageField = typeof(IniConfigManager<OsuSetting>).GetField("storage", instance)!;
        var precondition = isolation.GetMethod("ApiConstructorPrefix", BindingFlags.Static | BindingFlags.NonPublic)!;

        var harmonyAssembly = AppDomain.CurrentDomain.GetAssemblies().Single(a => a.GetName().Name == "0Harmony");
        var harmonyType = harmonyAssembly.GetType("HarmonyLib.Harmony", true)!;
        var harmonyMethodType = harmonyAssembly.GetType("HarmonyLib.HarmonyMethod", true)!;
        var testHarmony = Activator.CreateInstance(harmonyType, "illyaverse.proof.account-constructor")!;
        var testPrefix = Activator.CreateInstance(harmonyMethodType, typeof(AccountIsolationProof).GetMethod(nameof(pauseApiWorker), BindingFlags.Static | BindingFlags.NonPublic)!)!;
        var apiRun = typeof(APIAccess).GetMethod("run", instance)!;
        harmonyType.GetMethod("Patch")!.Invoke(testHarmony, [apiRun, testPrefix, null, null, null]);
        try
        {
            var game = (OsuGameBase)RuntimeHelpers.GetUninitializedObject(typeof(OsuGameBase));
            // Share the existing proof's mods bindable; the bridge refuses multiple game contexts.
            var relax = bridge.GetType("Illyaverse.Transfiguration.RelaxContext", true)!;
            typeof(OsuGameBase).GetField("SelectedMods", instance)!.SetValue(game,
                relax.GetField("mods", BindingFlags.Static | BindingFlags.NonPublic)!.GetValue(null));
            foreach (var expired in new[] { false, true })
            {
                if (File.Exists(privatePath)) File.Delete(privatePath);
                var officialToken = new OAuthToken
                {
                    AccessToken = "proof-official-access-never-sent",
                    AccessTokenExpiry = DateTimeOffset.UtcNow.AddSeconds(expired ? -3600 : 3600).ToUnixTimeSeconds(),
                    RefreshToken = "proof-official-refresh-never-sent"
                };
                File.WriteAllText(officialPath,
                    $"Username = proof-official-user\nToken = {officialToken}\nSavePassword = True\nSaveUsername = True\nAudioOffset = 123\n", Encoding.UTF8);
                var officialHash = SHA256.HashData(File.ReadAllBytes(officialPath));
                using var config = new OsuConfigManager(storage);
                require(config.Get<string>(OsuSetting.Username) == string.Empty && config.Get<string>(OsuSetting.Token) == string.Empty,
                    (expired ? "expired" : "valid") + " official username/access/refresh are absent before API construction");
                require(config.Get<double>(OsuSetting.AudioOffset) != 123, "official preferences are not copied with credentials");
                require(filename.Invoke(config, null) as string == "game.illyaverse.ini", "actual native filename getter selects private configuration");
                require(ReferenceEquals(storageField.GetValue(config), storage)
                    && storage.GetFullPath("client.realm") == realmPath && storage.GetFullPath("files/proof-library-resource") == resourcePath,
                    "native config retains the existing effective library Storage and resource paths");
                precondition.Invoke(null, [config, fixedEndpoints]);

                var unsafeConfig = (OsuConfigManager)RuntimeHelpers.GetUninitializedObject(typeof(OsuConfigManager));
                storageField.SetValue(unsafeConfig, storage);
                rejected(() => precondition.Invoke(null, [unsafeConfig, fixedEndpoints]), "config_not_isolated",
                    "untracked config is rejected before API token reads or worker startup");
                var workersBeforeRejection = pausedApiThreads;
                rejected(() => _ = new APIAccess(game, unsafeConfig, (EndpointConfiguration)fixedEndpoints, "proof-version"),
                    "config_not_isolated", "actual patched API constructor rejects an untracked shared-storage config");
                require(pausedApiThreads == workersBeforeRejection, "unsafe API constructor never reaches worker startup");
                var officialEndpoints = new EndpointConfiguration();
                rejected(() => precondition.Invoke(null, [config, officialEndpoints]), "api_config_not_private",
                    "official endpoint configuration is rejected before API construction");

                using var api = new APIAccess(game, config, (EndpointConfiguration)fixedEndpoints, "proof-version");
                require(apiWorkerStopped.WaitOne(TimeSpan.FromSeconds(2)), "test-only worker entry interception runs before persistence exercise");
                var authentication = (OAuth)typeof(APIAccess).GetField("authentication", instance)!.GetValue(api)!;
                require(api.ProvidedUsername == string.Empty && authentication.Token.Value is null && pausedApiThreads > 0,
                    "real API constructor receives no official credentials before test-only worker interception");
                var privateToken = new OAuthToken
                {
                    AccessToken = "proof-private-access",
                    AccessTokenExpiry = DateTimeOffset.UtcNow.AddSeconds(3600).ToUnixTimeSeconds(),
                    RefreshToken = "proof-private-refresh"
                };
                config.SetValue(OsuSetting.SavePassword, true);
                config.SetValue(OsuSetting.SaveUsername, true);
                config.SetValue(OsuSetting.Username, "proof-private-user");
                authentication.Token.Value = privateToken;
                require(config.Get<string>(OsuSetting.Token) == privateToken.ToString() && config.Save(),
                    "native remember-token handler and config save persist private credentials");
                using (var reloaded = new OsuConfigManager(storage))
                    require(reloaded.Get<string>(OsuSetting.Username) == "proof-private-user"
                        && reloaded.Get<string>(OsuSetting.Token) == privateToken.ToString()
                        && reloaded.Get<bool>(OsuSetting.SavePassword) && reloaded.Get<bool>(OsuSetting.SaveUsername),
                        "private credentials and remember flags reload from only the private file");

                config.SetValue(OsuSetting.SavePassword, false);
                authentication.Token.Value = new OAuthToken { AccessToken = "proof-private-second", AccessTokenExpiry = privateToken.AccessTokenExpiry, RefreshToken = "proof-private-second-refresh" };
                require(config.Get<string>(OsuSetting.Token) == string.Empty, "native remember-token-off handler clears persisted private token");
                config.SetValue(OsuSetting.SavePassword, true);
                authentication.Token.Value = privateToken;
                api.Logout();
                require(authentication.Token.Value is null && string.IsNullOrEmpty(config.Get<string>(OsuSetting.Token)) && config.Save(),
                    "native logout clears and saves only the private token");
                config.SetValue(OsuSetting.SaveUsername, false);
                config.SetValue(OsuSetting.Username, string.Empty);
                require(config.Save(), "private username and remember flags save independently");
                using (var loggedOut = new OsuConfigManager(storage))
                    require(loggedOut.Get<string>(OsuSetting.Token) == string.Empty
                        && loggedOut.Get<string>(OsuSetting.Username) == string.Empty
                        && !loggedOut.Get<bool>(OsuSetting.SaveUsername), "private logged-out state survives a real config reload");
                require(SHA256.HashData(File.ReadAllBytes(officialPath)).SequenceEqual(officialHash),
                    "official game.ini stays byte-identical through private save/reload/logout");
            }

            var unrelated = RuntimeHelpers.GetUninitializedObject(typeof(IniConfigManager<OsuSetting>));
            require(filename.Invoke(unrelated, null) as string == "game.ini", "same closed generic non-Osu manager retains stock filename");
            require(SHA256.HashData(File.ReadAllBytes(realmPath)).SequenceEqual(realmHash)
                && SHA256.HashData(File.ReadAllBytes(resourcePath)).SequenceEqual(resourceHash)
                && File.GetCreationTimeUtc(realmPath) == realmCreated && File.GetCreationTimeUtc(resourcePath) == resourceCreated,
                "existing Realm/resource marker files are not copied, replaced or modified");
            require(pausedApiThreads == 2, "only the two fixture APIAccess workers were intercepted");
            rejected(() => _ = new OsuConfigManager(new NativeStorage(root + "-foreign")), "config_storage_unverified",
                "actual config constructor rejects another storage before loading files");
            File.Delete(privatePath);
            Directory.CreateDirectory(privatePath);
            try
            {
                rejected(() => _ = new OsuConfigManager(storage), "config_file_unverified",
                    "directory at private filename is rejected before native config load");
            }
            finally { Directory.Delete(privatePath); }
        }
        finally
        {
            harmonyType.GetMethod("Unpatch", [typeof(MethodBase), typeof(MethodInfo)])!
                .Invoke(testHarmony, [apiRun, typeof(AccountIsolationProof).GetMethod(nameof(pauseApiWorker), BindingFlags.Static | BindingFlags.NonPublic)!]);
        }
        Console.WriteLine($"PASS {checks} real native config/constructor/persistence checks; API workers stopped at entry by test-only hook; no GameHost/Realm/GUI/real credentials/network");
        return checks;
    }
}
