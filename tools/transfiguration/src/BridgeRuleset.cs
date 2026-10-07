using osu.Game.Beatmaps;
using osu.Game.Rulesets;
using osu.Game.Rulesets.Difficulty;
using osu.Game.Rulesets.Mods;
using osu.Game.Rulesets.UI;

namespace osu.Game.Rulesets.Transfiguration;

// The launcher guard excludes this sentinel from the ruleset store. It is not a game mode.
// When a user leaves the DLL installed and starts lazer directly, its constructor fails closed.
public sealed class BridgeRuleset : Ruleset
{
    public BridgeRuleset()
    {
        if (!Illyaverse.Transfiguration.LaunchState.Active)
            Environment.FailFast("Transfiguration requires the Illyaverse launcher. Close lazer and remove the temporary Transfiguration DLL.");
    }

    public override string Description => "Illyaverse connection guard";
    public override string ShortName => "illyaverse-transfiguration";
    public override string RulesetAPIVersionSupported => "2026.818.0";
    public override IEnumerable<Mod> GetModsFor(ModType type) => Array.Empty<Mod>();
    public override DrawableRuleset CreateDrawableRulesetWith(IBeatmap beatmap, IReadOnlyList<Mod>? mods = null) => throw new NotSupportedException("Connection guard is not playable.");
    public override IBeatmapConverter CreateBeatmapConverter(IBeatmap beatmap) => throw new NotSupportedException("Connection guard is not playable.");
    public override DifficultyCalculator CreateDifficultyCalculator(IWorkingBeatmap beatmap) => throw new NotSupportedException("Connection guard is not playable.");
}
