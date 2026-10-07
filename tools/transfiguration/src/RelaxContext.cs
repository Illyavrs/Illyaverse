using System.Reflection;
using System.Runtime.CompilerServices;
using osu.Framework.Bindables;
using osu.Game;
using osu.Game.Online;
using osu.Game.Online.API;
using osu.Game.Online.API.Requests;
using osu.Game.Online.API.Requests.Responses;
using osu.Game.Rulesets;
using osu.Game.Rulesets.Mods;
using osu.Game.Users;

namespace Illyaverse.Transfiguration;

// Read-only API context: game ruleset IDs, submitted mods and writable requests are unchanged.
internal static class RelaxContext
{
    private const BindingFlags instance = BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic;
    private static readonly object gate = new();
    private static readonly ConditionalWeakTable<APIRequest, RequestScope> requests = new();
    private static readonly ConditionalWeakTable<LocalUserStatisticsProvider, StatisticsScope> statistics = new();
    private static readonly List<WeakReference<LocalUserStatisticsProvider>> providers = new();
    private static FieldInfo selectedMods = null!;
    private static FieldInfo cache = null!;
    private static PropertyInfo providerApi = null!;
    private static MethodInfo update = null!;
    private static MethodInfo schedule = null!;
    private static Bindable<IReadOnlyList<Mod>>? mods;
    private static IAPIProvider? capturedApi;
    private static bool relax;
    private static Mod? relaxMod;
    private static long generation;

    private sealed record RequestScope(long Generation, bool Relax, Mod? RelaxMod);
    private sealed class StatisticsScope(long initialGeneration)
    {
        internal long Generation = initialGeneration;
        internal long Serial;
        internal readonly Dictionary<string, RulesetInfo> Rulesets = new();
        internal readonly Dictionary<string, long> Latest = new();
    }

    internal static void VerifyContracts()
    {
        selectedMods = typeof(OsuGameBase).GetField("SelectedMods", instance)
            ?? throw new MissingFieldException("OsuGameBase", "SelectedMods");
        if (selectedMods.FieldType != typeof(Bindable<IReadOnlyList<Mod>>)) throw new InvalidOperationException("transfiguration_mod_bindable_incompatible");
        cache = typeof(LocalUserStatisticsProvider).GetField("statisticsCache", instance)!;
        providerApi = typeof(LocalUserStatisticsProvider).GetProperty("api", instance)!;
        update = typeof(LocalUserStatisticsProvider).GetMethod("UpdateStatistics", instance)!;
        schedule = typeof(IAPIProvider).GetMethod("Schedule", instance)!;
        if (cache?.FieldType != typeof(Dictionary<string, UserStatistics>) || providerApi?.PropertyType != typeof(IAPIProvider)
            || update?.ReturnType != typeof(void)
            || !update.GetParameters().Select(p => p.ParameterType).SequenceEqual([typeof(UserStatistics), typeof(RulesetInfo), typeof(Action<UserStatisticsUpdate>)])
            || schedule?.GetParameters().Single().ParameterType != typeof(Action))
            throw new InvalidOperationException("transfiguration_statistics_contract_incompatible");
        foreach (var type in new[] { typeof(GetScoresRequest), typeof(GetUserScoresRequest) })
            if (type.GetField("ruleset", instance)?.FieldType is not { } field || !typeof(IRulesetInfo).IsAssignableFrom(field))
                throw new InvalidOperationException("transfiguration_read_ruleset_contract_incompatible");
        if (typeof(GetUserRankingsRequest).BaseType!.GetField("ruleset", instance)?.FieldType != typeof(RulesetInfo))
            throw new InvalidOperationException("transfiguration_ranking_contract_incompatible");
        if (typeof(APIRequest).GetProperty("Uri", instance)?.PropertyType != typeof(string)
            || typeof(GetUserScoresRequest).GetField("type", instance)?.FieldType != typeof(ScoreType))
            throw new InvalidOperationException("transfiguration_read_request_contract_incompatible");
    }

    private static RequestScope snapshot()
    {
        lock (gate) return new(generation, relax, relaxMod);
    }

    private static bool eligible(APIRequest request) => request.GetType() == typeof(GetMeRequest)
        || request.GetType() == typeof(GetUserRequest) || request.GetType() == typeof(GetScoresRequest)
        || request.GetType() == typeof(GetUserScoresRequest) || request.GetType() == typeof(GetUserRankingsRequest);

    private static void CapturePrefix(APIAccess __instance, OsuGameBase game) => capture(__instance, game);

    // Also used by the isolated proof with an unconstructed game shell and a fake API provider.
    private static void capture(IAPIProvider api, OsuGameBase game)
    {
        var next = (Bindable<IReadOnlyList<Mod>>)selectedMods.GetValue(game)!;
        if (next is null) throw new InvalidOperationException("transfiguration_selected_mods_missing");
        lock (gate)
        {
            if (mods is not null && !ReferenceEquals(mods, next)) throw new InvalidOperationException("transfiguration_multiple_game_contexts");
            capturedApi = api;
            if (mods is not null) return;
            mods = next;
            relaxMod = next.Value.FirstOrDefault(m => m.Acronym == "RX");
            relax = relaxMod is not null;
            next.BindValueChanged(change => changed(change.NewValue));
        }
    }

    private static void changed(IReadOnlyList<Mod> value)
    {
        IAPIProvider? api;
        long epoch;
        lock (gate)
        {
            var next = value.FirstOrDefault(m => m.Acronym == "RX");
            var enabled = next is not null;
            relaxMod = next;
            if (enabled == relax) return;
            relax = enabled;
            epoch = ++generation;
            api = capturedApi;
        }
        if (api is not null) schedule.Invoke(api, [new Action(() => invalidate(epoch))]);
    }

    private static void RequestConstructorPrefix(APIRequest __instance)
    {
        if (eligible(__instance)) requests.GetValue(__instance, _ => snapshot());
    }

    private static string mode(APIRequest request) => request switch
    {
        GetMeRequest me => me.Ruleset?.ShortName ?? "osu",
        GetUserRequest user => user.Ruleset?.ShortName ?? "osu",
        GetScoresRequest scores => ((IRulesetInfo)scores.GetType().GetField("ruleset", instance)!.GetValue(scores)!).ShortName,
        GetUserScoresRequest scores => ((IRulesetInfo?)scores.GetType().GetField("ruleset", instance)!.GetValue(scores))?.ShortName ?? "osu",
        GetUserRankingsRequest rankings => ((RulesetInfo)rankings.GetType().BaseType!.GetField("ruleset", instance)!.GetValue(rankings)!).ShortName,
        _ => string.Empty
    };

    private static string variant(APIRequest request, RequestScope scope) => scope.Relax && mode(request) is "osu" or "taiko" or "fruits" or "catch" ? "relax" : "vanilla";

    private static void UriPostfix(APIRequest __instance, ref string __result)
    {
        if (!eligible(__instance)) return;
        if (__instance is GetUserScoresRequest scores && scores.GetType().GetField("type", instance)!.GetValue(scores)?.ToString() is not ("Best" or "Recent")) return;
        if (__instance is GetUserRankingsRequest ranking && ranking.Type != UserRankingsType.Performance) return;
        if (!Uri.TryCreate(__result, UriKind.Absolute, out var uri) || !NetworkPolicy.Allows(uri)
            || !uri.AbsolutePath.StartsWith("/api/v2/", StringComparison.Ordinal))
            throw new InvalidOperationException("transfiguration_read_endpoint_invalid");
        var scope = requests.GetValue(__instance, _ => snapshot());
        __result += (__result.Contains('?') ? "&" : "?") + "variant=" + variant(__instance, scope);
    }

    private static void ScoresConstructorPrefix(GetScoresRequest __instance, IRulesetInfo ruleset, ref IEnumerable<IMod>? mods)
    {
        if (__instance.GetType() != typeof(GetScoresRequest)) return;
        var scope = requests.GetValue(__instance, _ => snapshot());
        var supplied = mods?.ToArray() ?? [];
        // With no explicit filter, the variant alone selects the partition.
        if (supplied.Length == 0) return;
        var normalized = supplied.Where(m => m.Acronym != "RX").ToList();
        if (scope.Relax && ruleset.ShortName is "osu" or "taiko" or "fruits" or "catch")
        {
            normalized.RemoveAll(m => m.Acronym == "NM");
            normalized.Add(scope.RelaxMod ?? throw new InvalidOperationException("transfiguration_relax_mod_missing"));
        }
        mods = normalized.ToArray();
    }

    private static bool ScoresEqualsPrefix(GetScoresRequest __instance, GetScoresRequest? other, ref bool __result)
    {
        if (other is null || __instance.GetType() != typeof(GetScoresRequest) || other.GetType() != typeof(GetScoresRequest)) return true;
        if (variant(__instance, requests.GetValue(__instance, _ => snapshot())) == variant(other, requests.GetValue(other, _ => snapshot()))) return true;
        __result = false;
        return false;
    }

    private static StatisticsScope state(LocalUserStatisticsProvider provider)
    {
        lock (gate)
        {
            return statistics.GetValue(provider, key =>
            {
                providers.Add(new(key));
                return new(generation);
            });
        }
    }

    private static void StatisticsConstructorPostfix(LocalUserStatisticsProvider __instance) => state(__instance);

    private static bool StatisticsGetPrefix(LocalUserStatisticsProvider __instance, RulesetInfo ruleset, ref UserStatistics? __result)
    {
        lock (gate)
        {
            var scope = state(__instance);
            scope.Rulesets[ruleset.ShortName] = ruleset;
            if (scope.Generation == generation) return true;
            __result = null;
            return false;
        }
    }

    private static bool StatisticsRefetchPrefix(LocalUserStatisticsProvider __instance, RulesetInfo ruleset, Action<UserStatisticsUpdate>? callback)
    {
        refetch(__instance, ruleset, callback);
        return false;
    }

    private static void refetch(LocalUserStatisticsProvider provider, RulesetInfo ruleset, Action<UserStatisticsUpdate>? callback)
    {
        if (ruleset.OnlineID is < 0 or > 3) throw new InvalidOperationException("transfiguration_statistics_ruleset_invalid");
        var api = (IAPIProvider?)providerApi.GetValue(provider);
        if (api?.LocalUser.Value is not { Id: > 1 } localUser) return;
        RequestScope requestScope;
        StatisticsScope scope;
        long serial;
        var request = new GetUserRequest(localUser.Id, ruleset);
        lock (gate)
        {
            scope = state(provider);
            scope.Rulesets[ruleset.ShortName] = ruleset;
            if (scope.Generation != generation)
            {
                ((Dictionary<string, UserStatistics>)cache.GetValue(provider)!).Clear();
                scope.Generation = generation;
            }
            serial = ++scope.Serial;
            scope.Latest[ruleset.ShortName] = serial;
            requestScope = requests.GetValue(request, _ => snapshot());
        }
        request.Success += response =>
        {
            lock (gate)
            {
                if (requestScope.Generation != generation || scope.Generation != generation
                    || api.LocalUser.Value?.Id != localUser.Id || scope.Latest.GetValueOrDefault(ruleset.ShortName) != serial) return;
                update.Invoke(provider, [response.Statistics, ruleset, callback]);
            }
        };
        api.Queue(request);
    }

    private static void invalidate(long epoch)
    {
        List<(LocalUserStatisticsProvider Provider, RulesetInfo[] Rulesets)> active = new();
        lock (gate)
        {
            if (epoch != generation) return;
            for (var i = providers.Count - 1; i >= 0; i--)
            {
                if (!providers[i].TryGetTarget(out var provider)) { providers.RemoveAt(i); continue; }
                var scope = state(provider);
                ((Dictionary<string, UserStatistics>)cache.GetValue(provider)!).Clear();
                scope.Generation = epoch;
                scope.Latest.Clear();
                active.Add((provider, scope.Rulesets.Values.ToArray()));
            }
        }
        foreach (var (provider, rulesets) in active)
            foreach (var ruleset in rulesets)
            {
                // Notify stock consumers of the empty new partition while its data is fetched.
                var api = (IAPIProvider?)providerApi.GetValue(provider);
                if (api?.LocalUser.Value is not { } user) continue;
                update.Invoke(provider, [new UserStatistics { User = user }, ruleset, null]);
                refetch(provider, ruleset, null);
            }
    }
}
