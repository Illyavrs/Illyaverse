// Fixture behavior never exists in the production bridge.
public static class StartupHook
{
    public static void Initialize()
    {
        if (Environment.GetEnvironmentVariable("ILLYA_FIXTURE_THROW") == "1")
            throw new InvalidOperationException("Intentional headless startup refusal");
        File.WriteAllText(Environment.GetEnvironmentVariable("ILLYA_FIXTURE_HOOK_MARKER")!, "hook");
    }
}
