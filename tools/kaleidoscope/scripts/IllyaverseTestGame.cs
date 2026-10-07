// Original Illyaverse test-client adapter. Compiled only inside a pinned upstream checkout.
using osu.Game.IO;
using osu.Game.Online;
using osu.Game.Updater;

namespace osu.Desktop;

internal partial class IllyaverseTestGame : OsuGameDesktop
{
    public IllyaverseTestGame(string[] args) : base(args) { }

    public override EndpointConfiguration CreateEndpoints() => new()
    {
        WebsiteUrl = "https://illya.test",
        APIUrl = "https://osu.illya.test",
        APIClientID = "illyaverse-local-client",
        APIClientSecret = "illyaverse-public-test-client",
        SpectatorUrl = "https://spectator.illya.test/spectator",
        MultiplayerUrl = "https://spectator.illya.test/multiplayer",
        MetadataUrl = "https://spectator.illya.test/metadata",
        BeatmapSubmissionServiceUrl = null,
        LivenessProbeUrl = null,
    };

    // Keep update checks and stable's installed data outside the isolated test session.
    protected override UpdateManager CreateUpdateManager() => new();
    public override StableStorage? GetStorageForStableInstall() => null;
}
